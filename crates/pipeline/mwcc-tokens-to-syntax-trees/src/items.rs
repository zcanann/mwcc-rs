//! Parsing of types, functions, parameters, locals, and guarded returns.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{Expression, Function, GlobalDeclaration, GuardedReturn, LocalDeclaration, LoopKind, Parameter, Pointee, PointerElement, Statement, SwitchArm, TranslationUnit, Type};
use mwcc_tokens::Token;

use crate::parser::{Parser, StructField, StructLayout};

/// `target` assigned `value`: a reassignment of a tracked local, or a memory
/// store to any other lvalue (`*p`, `p[i]`, a member, a global).
/// A `static` local parsed out of a SKIPPED inline definition.
struct SkippedStaticLocal {
    name: String,
    declared_type: Type,
    is_const: bool,
    /// The byte image; `None` = zero-initialized (.sbss).
    bytes: Option<Vec<u8>>,
    byte_size: u16,
}

fn store_or_assign(target: Expression, value: Expression, local_names: &std::collections::HashSet<String>) -> Statement {
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
        Type::LongLong => Ok(Pointee::LongLong),
        Type::UnsignedLongLong => Ok(Pointee::UnsignedLongLong),
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
/// Pack a bit-field value into `image` at storage-unit byte `unit_base`:
/// `bit_offset` counts from the unit's most-significant end (big-endian).
fn pack_bit_field(image: &mut [u8], unit_base: usize, bit_offset: u8, width: u8, value: u64) {
    for bit in 0..width {
        let source = (value >> (width - 1 - bit)) & 1;
        let absolute = bit_offset as usize + bit as usize;
        let byte = unit_base + absolute / 8;
        image[byte] |= (source as u8) << (7 - (absolute % 8));
    }
}

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

/// Whether an expression tree contains a call to any of `names`
/// (the inline-materialization and skipped-inline checks share this walk).
    fn expression_calls(expression: &Expression, names: &std::collections::HashSet<String>) -> bool {
        match expression {
            Expression::Call { name, arguments } => {
                names.contains(name) || arguments.iter().any(|argument| expression_calls(argument, names))
            }
            Expression::Binary { left, right, .. } => {
                expression_calls(left, names) || expression_calls(right, names)
            }
            Expression::Unary { operand, .. }
            | Expression::Cast { operand, .. }
            | Expression::AddressOf { operand } => expression_calls(operand, names),
            Expression::Dereference { pointer } => expression_calls(pointer, names),
            Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => expression_calls(base, names),
            Expression::Index { base, index } => {
                expression_calls(base, names) || expression_calls(index, names)
            }
            Expression::Assign { target, value } => {
                expression_calls(target, names) || expression_calls(value, names)
            }
            Expression::Conditional { condition, when_true, when_false } => {
                expression_calls(condition, names)
                    || expression_calls(when_true, names)
                    || expression_calls(when_false, names)
            }
            _ => false,
        }
    }
    fn statement_calls(statement: &Statement, names: &std::collections::HashSet<String>) -> bool {
        match statement {
            Statement::Store { target, value } => {
                expression_calls(target, names) || expression_calls(value, names)
            }
            Statement::Assign { value, .. } => expression_calls(value, names),
            Statement::Expression(expression) => expression_calls(expression, names),
            Statement::If { condition, then_body, else_body } => {
                expression_calls(condition, names)
                    || then_body.iter().any(|inner| statement_calls(inner, names))
                    || else_body.iter().any(|inner| statement_calls(inner, names))
            }
            Statement::Switch { scrutinee, arms, default } => {
                expression_calls(scrutinee, names)
                    || arms.iter().any(|arm| match &arm.body {
                mwcc_syntax_trees::ArmBody::Return(result) => expression_calls(result, names),
                mwcc_syntax_trees::ArmBody::Statements(statements) => {
                    statements.iter().any(|statement| statement_calls(statement, names))
                }
            })
                    || default.as_ref().is_some_and(|body| match body {
                        mwcc_syntax_trees::ArmBody::Return(expression) => expression_calls(expression, names),
                        mwcc_syntax_trees::ArmBody::Statements(statements) => {
                            statements.iter().any(|statement| statement_calls(statement, names))
                        }
                    })
            }
            Statement::Return(Some(expression)) => expression_calls(expression, names),
            Statement::Loop { initializer, condition, step, body, .. } => {
                initializer.as_ref().is_some_and(|expression| expression_calls(expression, names))
                    || condition.as_ref().is_some_and(|expression| expression_calls(expression, names))
                    || step.as_ref().is_some_and(|expression| expression_calls(expression, names))
                    || body.iter().any(|inner| statement_calls(inner, names))
            }
            _ => false,
        }
    }
    // A call to a skipped inline is recorded on the unit — codegen
    // defers such functions AFTER the exact-match templates get a
    // claim (a whole-function capture already has the inline
    // flattened into its body).

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
    fn parse_switch(&mut self, local_names: &mut std::collections::HashSet<String>, block_locals: &mut Vec<LocalDeclaration>) -> Compilation<Statement> {
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
                let (body, falls_through) = self.parse_switch_arm_body(local_names, block_locals)?;
                arms.push(SwitchArm { value, body, falls_through });
            } else if self.eat_word("default") {
                self.expect(Token::Colon)?;
                let (body, _falls_through) = self.parse_switch_arm_body(local_names, block_locals)?;
                default = Some(body);
            } else if matches!(self.peek(), Token::Identifier(_)) && *self.peek_at(1) == Token::Colon {
                // A goto LABEL between arms (scanf's `signed_int:`) — control
                // reaches it by falling through the previous arm or by goto, so
                // the label and its statements continue that arm's body.
                let name = self.parse_identifier()?;
                self.advance(); // the colon
                let (continuation, falls_through) = self.parse_switch_arm_body(local_names, block_locals)?;
                let Some(last) = arms.last_mut() else {
                    return Err(Diagnostic::error("a goto label before the first switch arm is not supported yet (roadmap)"));
                };
                let mut statements = match std::mem::replace(&mut last.body, mwcc_syntax_trees::ArmBody::Statements(Vec::new())) {
                    mwcc_syntax_trees::ArmBody::Return(expression) => vec![Statement::Return(Some(expression))],
                    mwcc_syntax_trees::ArmBody::Statements(statements) => statements,
                };
                statements.push(Statement::Label(name));
                match continuation {
                    mwcc_syntax_trees::ArmBody::Return(expression) => statements.push(Statement::Return(Some(expression))),
                    mwcc_syntax_trees::ArmBody::Statements(inner) => statements.extend(inner),
                }
                last.body = mwcc_syntax_trees::ArmBody::Statements(statements);
                last.falls_through = falls_through;
            } else {
                return Err(Diagnostic::error("a switch arm must be `case <int>: return …;` or `default: return …;` (roadmap)"));
            }
        }
        self.expect(Token::BraceClose)?;
        Ok(Statement::Switch { scrutinee, arms, default })
    }

    /// A switch arm's body: the common `return E;` (optionally braced, with
    /// dead trailing `break;`s), or a braced STATEMENT body ending at its
    /// `break;` — represented faithfully (mwcc branches these; a ternary
    /// lowering is byte-different).
    fn parse_switch_arm_body(&mut self, local_names: &mut std::collections::HashSet<String>, block_locals: &mut Vec<LocalDeclaration>) -> Compilation<(mwcc_syntax_trees::ArmBody, bool)> {
        use mwcc_syntax_trees::ArmBody;
        let braced = self.eat_keyword(Token::BraceOpen);
        if *self.peek() == Token::KeywordReturn {
            self.advance();
            let result = self.expression()?;
            self.expect(Token::Semicolon)?;
            if braced {
                while matches!(self.peek(), Token::Identifier(word) if word == "break") {
                    self.advance();
                    self.expect(Token::Semicolon)?;
                }
                self.expect(Token::BraceClose)?;
            }
            return Ok((ArmBody::Return(result), false));
        }
        // A statement body: if-statements and returns, ending at `break;`
        // (unbraced arms also end at the next case/default label or the
        // switch's closing brace). An arm ending WITHOUT break/return falls
        // through — an empty body is a shared label (`case 'd': case 'i':`).
        let mut statements: Vec<Statement> = Vec::new();
        let mut saw_break = false;
        loop {
            if matches!(self.peek(), Token::Identifier(word) if word == "break") {
                self.advance();
                self.expect(Token::Semicolon)?;
                saw_break = true;
                if braced {
                    continue; // dead after full-return diamonds; end-of-arm otherwise
                }
                break; // an unbraced arm ends at its break
            }
            if !braced
                && matches!(self.peek(), Token::Identifier(word) if word == "case" || word == "default")
            {
                break; // fall into the next label's arm boundary
            }
            if !braced && *self.peek() == Token::BraceClose {
                break; // the switch's closing brace
            }
            if braced && *self.peek() == Token::BraceClose {
                self.advance();
                break;
            }
            if *self.peek() == Token::KeywordIf {
                statements.push(self.parse_if_statement(local_names, block_locals)?);
                continue;
            }
            if *self.peek() == Token::KeywordReturn {
                statements.push(self.parse_return_statement()?);
                continue;
            }
            if matches!(self.peek(), Token::KeywordWhile | Token::KeywordDo | Token::KeywordFor) {
                statements.push(self.parse_loop_statement(local_names, block_locals)?);
                continue;
            }
            // A bare `{ ... }` scoping block inside an arm flattens like one in
            // a function body (its declarations hoist with the block's).
            if *self.peek() == Token::BraceOpen {
                let mut inner = self.parse_block(local_names, block_locals)?;
                statements.append(&mut inner);
                continue;
            }
            if let Some(statement) = self.parse_jump_statement()? {
                statements.push(statement);
                continue;
            }
            statements.push(self.parse_simple_statement(local_names, block_locals)?);
        }
        let falls_through =
            !saw_break && !matches!(statements.last(), Some(Statement::Return(_) | Statement::Goto(_)));
        Ok((ArmBody::Statements(statements), falls_through))
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
            // `&g[0]` addresses the first element — the same relocation as `&g`
            // (ansi_files' `FILE* p = &__files[0];`). A nonzero index needs an
            // addend model on PointerElement; defer until measured.
            if *self.peek() == Token::BracketOpen && self.peek_at(1) == &Token::IntegerLiteral(0) && self.peek_at(2) == &Token::BracketClose {
                self.advance();
                self.advance();
                self.advance();
            }
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
                // A float/double initializer is a constant EXPRESSION evaluated in double
                // (`M_PI / 180`, `1.0f / 3.0f`, a bare literal, `-1.5f`), then narrowed to
                // the element width. Parse and fold it rather than accepting only a single
                // literal.
                let expression = self.expression()?;
                let value = crate::expressions::fold_constant_float(&expression)?;
                Ok(if element_type == Type::Float { (value as f32).to_bits() as i64 } else { value.to_bits() as i64 })
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
        let mut relocations = Vec::new();
        let bytes = self.parse_one_struct_relocated(tag, 0, &mut relocations)?;
        if !relocations.is_empty() {
            return Err(Diagnostic::error("an address element in this initializer position is not supported yet (roadmap)"));
        }
        Ok(bytes)
    }

    /// One BRACED struct value for layout `tag`, written into a fresh byte image.
    /// `base_offset` positions the value inside the enclosing data object so
    /// ADDRESS elements (`__read_console`, `(char*)&(&__files[0])->field`) record
    /// `(object offset, target, addend)` relocations. Values map to fields with
    /// C89 FLAT DESCENT: an unbraced nested struct/array consumes values for its
    /// own members from the same list; bit-fields pack into their storage units.
    fn parse_one_struct_relocated(&mut self, tag: &str, base_offset: u32, relocations: &mut Vec<(u32, String, i32)>) -> Compilation<Vec<u8>> {
        let struct_size = {
            let layout = self.structs.get(tag).ok_or_else(|| Diagnostic::error(format!("struct '{tag}' is not declared")))?;
            layout.size
        };
        let mut bytes = vec![0u8; struct_size as usize];
        self.expect(Token::BraceOpen)?;
        self.fill_struct_fields(tag, &mut bytes, 0, base_offset, relocations)?;
        self.eat_keyword(Token::Comma);
        self.expect(Token::BraceClose)?;
        Ok(bytes)
    }

    /// The declaration-ordered field list for `tag`: (offset, type, nested tag,
    /// element pointee, total array bytes, bit-field). Bit-fields sharing a
    /// storage unit order by their bit offset.
    fn ordered_struct_fields(&self, tag: &str) -> Compilation<Vec<(u16, Type, Option<String>, Option<Pointee>, Option<u16>, Option<(u8, u8)>)>> {
        let layout = self.structs.get(tag).ok_or_else(|| Diagnostic::error(format!("struct '{tag}' is not declared")))?;
        let mut ordered: Vec<_> = layout
            .fields
            .values()
            .map(|field| (field.offset, field.member_type, field.struct_tag.clone(), field.array_element, field.array_bytes, field.bit_field))
            .collect();
        ordered.sort_by_key(|(offset, _, _, _, _, bit_field)| (*offset, bit_field.map_or(0, |(bit, _)| bit)));
        Ok(ordered)
    }

    /// Fill `tag`'s fields from the value list at the cursor into `image`
    /// starting at `struct_base`, stopping early at `}` (remaining fields stay
    /// zero). Consumes one trailing comma after each value; consumes NO braces
    /// itself (the caller owns the enclosing pair; a BRACED sub-aggregate is
    /// detected per field).
    fn fill_struct_fields(&mut self, tag: &str, image: &mut [u8], struct_base: usize, absolute_base: u32, relocations: &mut Vec<(u32, String, i32)>) -> Compilation<()> {
        let fields = self.ordered_struct_fields(tag)?;
        for (offset, member_type, nested_tag, array_element, array_bytes, bit_field) in fields {
            if *self.peek() == Token::BraceClose {
                break;
            }
            let field_base = struct_base + offset as usize;
            let absolute_field = absolute_base + offset as u32;
            if let Some((bit_offset, width)) = bit_field {
                let value = self.parse_scalar_constant(Type::UnsignedInt)?;
                pack_bit_field(image, field_base, bit_offset, width, value as u64);
            } else if let (Some(nested), Type::Struct { .. }) = (nested_tag.as_ref(), member_type) {
                // Only a struct VALUE field descends — a struct POINTER carries
                // the tag too (for member resolution) but is a 4-byte address.
                let nested = nested.clone();
                if *self.peek() == Token::BraceOpen {
                    self.advance();
                    self.fill_struct_fields(&nested, image, field_base, absolute_field, relocations)?;
                    self.eat_keyword(Token::Comma);
                    self.expect(Token::BraceClose)?;
                } else {
                    // FLAT descent: the nested struct's fields consume from this
                    // list — including each value's trailing comma, so the outer
                    // loop must NOT eat another separator.
                    self.fill_struct_fields(&nested, image, field_base, absolute_field, relocations)?;
                    continue;
                }
            } else if let Some(total) = array_bytes {
                let element_width = array_element.map_or(4, |element| element.size() as usize).max(1);
                let count = (total as usize / element_width).max(1);
                let element_type = array_element.map_or(Type::UnsignedInt, |element| element.element());
                let braced = self.eat_keyword(Token::BraceOpen);
                for index in 0..count {
                    if *self.peek() == Token::BraceClose {
                        break;
                    }
                    let value = self.parse_scalar_constant(element_type)?;
                    let at = field_base + index * element_width;
                    let encoded = (value as u64).to_be_bytes();
                    image[at..at + element_width].copy_from_slice(&encoded[8 - element_width..]);
                    if !self.eat_keyword(Token::Comma) {
                        break;
                    }
                }
                if braced {
                    self.expect(Token::BraceClose)?;
                    self.eat_keyword(Token::Comma);
                }
                continue; // the comma after an unbraced array's last element is consumed above
            } else if matches!(member_type, Type::Pointer(_) | Type::StructPointer { .. }) {
                // A pointer field initialized with a STRING LITERAL pools the
                // string as an anonymous `@N` `.sdata` object (locale's lconv:
                // `{".", "", ...}` — main.rs assigns the numbers in first-
                // appearance order, deduplicated under `-str reuse`). The
                // marker prefix routes it through the symbol-relocation tuple.
                if let Token::StringLiteral(string_bytes) = self.peek() {
                    let marker = format!("\u{1}{}", String::from_utf8_lossy(string_bytes));
                    self.advance();
                    relocations.push((absolute_field, marker, 0));
                    if !self.eat_keyword(Token::Comma) {
                        break;
                    }
                    continue;
                }
                // A pointer field: a relocated address (`__read_console`, a cast
                // `&member` chain into a struct array) or a constant (NULL).
                if let Some((target, addend)) = self.parse_address_element(tag)? {
                    relocations.push((absolute_field, target, addend));
                    // the image bytes stay zero — the relocation fills them at link time
                } else {
                    let value = self.parse_scalar_constant(Type::UnsignedInt)?;
                    let encoded = (value as u64).to_be_bytes();
                    image[field_base..field_base + 4].copy_from_slice(&encoded[4..]);
                }
            } else {
                let value = self.parse_scalar_constant(member_type)?;
                let width = type_size(member_type) as usize;
                let encoded = (value as u64).to_be_bytes();
                image[field_base..field_base + width].copy_from_slice(&encoded[8 - width..]);
            }
            if !self.eat_keyword(Token::Comma) {
                break;
            }
        }
        Ok(())
    }

    /// Classify a pointer-field initializer element at the cursor. Returns
    /// `Some((symbol, addend))` for the relocated-address forms — a bare
    /// function/global name, or a (possibly cast) `&<lvalue>` chain resolving
    /// into a struct-array global (`(char*)&((&__files[0]))->field` — measured:
    /// ansi_files' self-referential FILE table). Returns `None` (cursor
    /// unmoved) when the element is an ordinary constant expression.
    fn parse_address_element(&mut self, tag: &str) -> Compilation<Option<(String, i32)>> {
        let start = self.position;
        // optional cast(s): `(char*)`, `(void*)` ... skip `( type * )` groups.
        loop {
            if *self.peek() == Token::ParenOpen && self.token_starts_type(self.peek_at(1)) {
                // find the matching close; a cast group is short — scan it
                let mut index = self.position + 1;
                let mut depth = 1;
                while depth > 0 {
                    match self.tokens.get(index) {
                        Some(Token::ParenOpen) => depth += 1,
                        Some(Token::ParenClose) => depth -= 1,
                        None => break,
                        _ => {}
                    }
                    index += 1;
                }
                // Only skip when a cast is followed by MORE expression (not `(void*)0`'s
                // constant fold — that path returns None below and re-parses).
                if matches!(self.tokens.get(index), Some(Token::Ampersand) | Some(Token::Identifier(_))) {
                    self.position = index;
                    continue;
                }
            }
            break;
        }
        // Bare `name` followed by `,` or `}` — a function/global address.
        if let (Token::Identifier(name), Some(Token::Comma) | Some(Token::BraceClose)) = (self.peek(), self.tokens.get(self.position + 1)) {
            if !self.enum_constants.contains_key(name) {
                let name = name.clone();
                self.advance();
                return Ok(Some((name, 0)));
            }
        }
        // `name + K` — an address with a byte addend into a known global array
        // (pikmin's locale: `stringBase0 + 2` indexes the packed string table).
        if let (Token::Identifier(name), Some(Token::Plus), Some(Token::IntegerLiteral(addend))) =
            (self.peek(), self.tokens.get(self.position + 1), self.tokens.get(self.position + 2))
        {
            if self.global_sizes.contains_key(name)
                && matches!(self.tokens.get(self.position + 3), Some(Token::Comma) | Some(Token::BraceClose))
            {
                let name = name.clone();
                let addend = *addend as i32;
                self.position += 3;
                return Ok(Some((name, addend)));
            }
        }
        // `&name` — an explicit address-of a function/global (mp4's
        // `&__read_console`); same relocation as the bare-name form.
        if *self.peek() == Token::Ampersand {
            if let Some(Token::Identifier(name)) = self.tokens.get(self.position + 1) {
                if matches!(self.tokens.get(self.position + 2), Some(Token::Comma) | Some(Token::BraceClose)) {
                    let name = name.clone();
                    self.position += 2;
                    return Ok(Some((name, 0)));
                }
            }
        }
        // `&global.member[.member…]` — a (possibly nested) member address in a
        // STRUCT-typed global (`&__files._stdout`, `&__files._stdin.char_buffer`);
        // each step's offset comes from the successive layouts.
        if *self.peek() == Token::Ampersand {
            if let (Some(Token::Identifier(global)), Some(Token::Dot)) =
                (self.tokens.get(self.position + 1), self.tokens.get(self.position + 2))
            {
                if let Some(outer_tag) = self.global_structs.get(global) {
                    let global = global.clone();
                    let mut current_tag = outer_tag.clone();
                    let mut addend: i32 = 0;
                    let mut cursor = self.position + 2;
                    loop {
                        if self.tokens.get(cursor) != Some(&Token::Dot) {
                            break;
                        }
                        let Some(Token::Identifier(member)) = self.tokens.get(cursor + 1) else {
                            break;
                        };
                        let layout = self.structs.get(&current_tag).ok_or_else(|| Diagnostic::error(format!("struct '{current_tag}' is not declared")))?;
                        let Some(field) = layout.fields.get(member) else {
                            break;
                        };
                        addend += field.offset as i32;
                        let next_tag = field.struct_tag.clone();
                        cursor += 2;
                        if matches!(self.tokens.get(cursor), Some(Token::Comma) | Some(Token::BraceClose)) {
                            self.position = cursor;
                            return Ok(Some((global, addend)));
                        }
                        match next_tag {
                            Some(tag) => current_tag = tag,
                            None => break,
                        }
                    }
                }
            }
        }
        // `&global[i]` — the address of a whole array element (ansi_files'
        // mNextFile chain in the mp4/AC variants).
        if *self.peek() == Token::Ampersand {
            if let Some(Token::Identifier(global)) = self.tokens.get(self.position + 1) {
                if self.tokens.get(self.position + 2) == Some(&Token::BracketOpen) {
                    if let (Some(Token::IntegerLiteral(element)), Some(Token::BracketClose)) =
                        (self.tokens.get(self.position + 3), self.tokens.get(self.position + 4))
                    {
                        if matches!(self.tokens.get(self.position + 5), Some(Token::Comma) | Some(Token::BraceClose)) {
                            let global = global.clone();
                            let element = *element;
                            let layout_size = self
                                .structs
                                .get(tag)
                                .ok_or_else(|| Diagnostic::error(format!("struct '{tag}' is not declared")))?
                                .size;
                            self.position += 5;
                            return Ok(Some((global, element as i32 * layout_size as i32)));
                        }
                    }
                }
            }
        }
        // `&( (&global[i]) )->field` — resolve through the CURRENT tag's layout.
        if *self.peek() == Token::Ampersand {
            let mut index = self.position + 1;
            while self.tokens.get(index) == Some(&Token::ParenOpen) {
                index += 1;
            }
            if self.tokens.get(index) == Some(&Token::Ampersand) {
                if let Some(Token::Identifier(global)) = self.tokens.get(index + 1) {
                    let global = global.clone();
                    if self.tokens.get(index + 2) == Some(&Token::BracketOpen) {
                        if let Some(Token::IntegerLiteral(element)) = self.tokens.get(index + 3) {
                            let element = *element;
                            let mut cursor = index + 4;
                            if self.tokens.get(cursor) == Some(&Token::BracketClose) {
                                cursor += 1;
                                while self.tokens.get(cursor) == Some(&Token::ParenClose) {
                                    cursor += 1;
                                }
                                if matches!(self.tokens.get(cursor), Some(Token::Arrow) | Some(Token::Dot)) {
                                    if let Some(Token::Identifier(field)) = self.tokens.get(cursor + 1) {
                                        let layout = self.structs.get(tag).ok_or_else(|| Diagnostic::error(format!("struct '{tag}' is not declared")))?;
                                        let field_offset = layout
                                            .fields
                                            .get(field)
                                            .map(|entry| entry.offset)
                                            .ok_or_else(|| Diagnostic::error(format!("no member '{field}' in struct '{tag}'")))?;
                                        let addend = element as i32 * layout.size as i32 + field_offset as i32;
                                        self.position = cursor + 2;
                                        return Ok(Some((global, addend)));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let _ = start;
            return Err(Diagnostic::error("this address-of initializer shape is not supported yet (roadmap)"));
        }
        Ok(None)
    }

    /// Parse a `{ s0, s1, ... }` array of struct values for the layout `tag`, each
    /// element parsed by [`Self::parse_one_struct`] and concatenated (the array stride
    /// is the struct size, which each element's image already fills).
    fn parse_struct_array_initializer(&mut self, tag: &str, relocations: &mut Vec<(u32, String, i32)>) -> Compilation<Vec<u8>> {
        self.expect(Token::BraceOpen)?;
        let mut bytes = Vec::new();
        while *self.peek() != Token::BraceClose {
            let element = self.parse_one_struct_relocated(tag, bytes.len() as u32, relocations)?;
            bytes.extend(element);
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
            let tagged = matches!(self.peek(), Token::Identifier(_));
            if tagged {
                self.advance(); // the tag
            }
            if *self.peek() == Token::BraceOpen {
                // An ANONYMOUS enum definition consumes one anonymous-`@N` number
                // (measured fire 494: `typedef enum {…} E;` shifts the next pool
                // constant by +1; a TAGGED enum adds nothing — pikmin's uart TU
                // carries three such enums between its inlines and its statics).
                // Keyed by token position so a speculative re-parse can't double-count.
                if !tagged && self.counted_enum_positions.insert(self.position) {
                    self.skipped_inline_functions += 1;
                }
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
            if *self.peek() == Token::Star {
                // `S**` — a pointer to a struct pointer: a word-classed
                // pointer whose element is itself a pointer.
                self.advance();
                return Ok(Type::Pointer(Pointee::Pointer));
            }
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
            if *self.peek() == Token::Star {
                // `S**` — a pointer to a struct pointer: a word-classed
                // pointer whose element is itself a pointer.
                self.advance();
                return Ok(Type::Pointer(Pointee::Pointer));
            }
            return Ok(Type::StructPointer { element_size });
        }
        // A struct-pointer typedef (`VecPtr`) is itself a pointer to the struct —
        // no trailing `*` — carrying the layout's tag.
        if let Token::Identifier(name) = self.peek() {
            if let Some(tag) = self.struct_pointer_typedefs.get(name).cloned() {
                self.advance();
                let element_size = self.structs.get(&tag).map_or(0, |layout| layout.size);
                self.last_struct_tag = Some(tag);
                if *self.peek() == Token::Star {
                    self.advance();
                    return Ok(Type::Pointer(Pointee::Pointer));
                }
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
                if *self.peek() == Token::Star {
                    self.advance();
                    return Ok(Type::Pointer(Pointee::Pointer));
                }
                return Ok(Type::StructPointer { element_size });
            }
        }
        // A `typedef`-declared alias resolves to its underlying type.
        if let Token::Identifier(name) = self.peek() {
            if let Some(&aliased) = self.typedefs.get(name) {
                self.advance();
                if *self.peek() == Token::Star {
                    self.advance();
                    // A star on an already-pointer typedef (`voidfunctionptr*`)
                    // is a pointer-to-pointer: word element, inner untracked.
                    if matches!(aliased, Type::Pointer(_) | Type::StructPointer { .. }) {
                        return Ok(Type::Pointer(Pointee::Pointer));
                    }
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
        // East-const/volatile: a qualifier may TRAIL the base type (`float const`, the
        // mirror of the leading `const float` — dolphin/MSL headers use both). Fold
        // `const` into `last_type_was_const` so the global path still sees it as read-only.
        self.consume_trailing_qualifiers();
        // A trailing `*` makes it a pointer to that scalar; a qualifier after the `*`
        // is a const/volatile POINTER (`int *const p`), also transparent to codegen.
        if *self.peek() == Token::Star {
            self.advance();
            self.consume_trailing_qualifiers();
            // A SECOND `*` is a pointer-to-pointer (`char **end`): word-sized
            // element, inner pointee untracked (double derefs defer at codegen).
            if *self.peek() == Token::Star {
                self.advance();
                self.consume_trailing_qualifiers();
                return Ok(Type::Pointer(Pointee::Pointer));
            }
            return Ok(Type::Pointer(pointee_of(base)?));
        }
        Ok(base)
    }

    /// Consume a run of `const`/`volatile`/`register` qualifiers that TRAIL a type (east
    /// const), noting `const`/`volatile` in the same flags the leading `skip_type_qualifiers`
    /// sets — but never resetting them, so a leading qualifier already seen is preserved.
    pub(crate) fn consume_trailing_qualifiers(&mut self) {
        loop {
            match self.peek() {
                Token::Identifier(word) if word == "const" => {
                    self.last_type_was_const = true;
                    self.advance();
                }
                Token::Identifier(word) if word == "volatile" => {
                    self.last_type_was_volatile = true;
                    self.advance();
                }
                Token::Identifier(word) if word == "register" => {
                    self.advance();
                }
                _ => return,
            }
        }
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
                        layout.fields.insert(name, StructField { member_type: Type::Struct { size: inner_size, align: inner_align as u8 }, offset, struct_tag: Some(tag), array_element: None, array_bytes: None, bit_field: None });
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
                        layout.fields.insert(name, StructField { member_type: Type::Struct { size: inner_size, align: inner_align as u8 }, offset, struct_tag: Some(synthetic), array_element: None, array_bytes: None, bit_field: None });
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
                                array_bytes: field.array_bytes,
                                bit_field: field.bit_field,
                            });
                        }
                        offset += inner_size;
                    }
                }
                self.expect(Token::Semicolon)?;
                if let Some((_, unit_offset, bits_used)) = bit_unit.take() {
                    // mwcc TRIMS the container to the bytes its bits use
                    // (measured: 4 bits -> next byte member at +1; 9-12 bits
                    // -> +2; the container type still sets the alignment).
                    offset = unit_offset + (bits_used as u16).div_ceil(8);
                }
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
                if let Some((_, unit_offset, bits_used)) = bit_unit.take() {
                    // mwcc TRIMS the container to the bytes its bits use
                    // (measured: 4 bits -> next byte member at +1; 9-12 bits
                    // -> +2; the container type still sets the alignment).
                    offset = unit_offset + (bits_used as u16).div_ceil(8);
                }
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
                                array_bytes: field.array_bytes,
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
                    if let Some((_, unit_offset, bits_used)) = bit_unit.take() {
                    // mwcc TRIMS the container to the bytes its bits use
                    // (measured: 4 bits -> next byte member at +1; 9-12 bits
                    // -> +2; the container type still sets the alignment).
                    offset = unit_offset + (bits_used as u16).div_ceil(8);
                }
                    alignment_max = alignment_max.max(alignment);
                    offset = offset.div_ceil(alignment) * alignment;
                    layout.fields.insert(field_name, StructField { member_type: element, offset, struct_tag: None, array_element: Some(pointee_of(element)?), array_bytes: Some(count.saturating_mul(element_size)), bit_field: None });
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
                    if let Some((_, unit_offset, bits_used)) = bit_unit.take() {
                    // mwcc TRIMS the container to the bytes its bits use
                    // (measured: 4 bits -> next byte member at +1; 9-12 bits
                    // -> +2; the container type still sets the alignment).
                    offset = unit_offset + (bits_used as u16).div_ceil(8);
                }
                    let alignment = 4u16;
                    alignment_max = alignment_max.max(alignment);
                    offset = offset.div_ceil(alignment) * alignment;
                    layout.fields.insert(pointer_name, StructField { member_type: Type::StructPointer { element_size: 0 }, offset, struct_tag: None, array_element: None, array_bytes: None, bit_field: None });
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
                    layout.fields.insert(field_name, StructField { member_type: field_type, offset: unit_offset, struct_tag: None, array_element: None, array_bytes: None, bit_field: Some((bit_offset, width)) });
                    if !self.eat_keyword(Token::Comma) {
                        break;
                    }
                    continue;
                }
                // An ordinary member closes any open bit-field unit.
                if let Some((_, unit_offset, bits_used)) = bit_unit.take() {
                    // mwcc TRIMS the container to the bytes its bits use
                    // (measured: 4 bits -> next byte member at +1; 9-12 bits
                    // -> +2; the container type still sets the alignment).
                    offset = unit_offset + (bits_used as u16).div_ceil(8);
                }
                // An array member `type name[N]` occupies `N` elements; its access
                // yields the array address rather than a loaded value.
                let mut array_element = None;
                let mut is_array = false;
                let mut size = type_size(field_type);
                let element_size = size;
                if *self.peek() == Token::BracketOpen {
                    is_array = true;
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
                layout.fields.insert(field_name, StructField { member_type: field_type, offset, struct_tag: struct_tag.clone(), array_element, array_bytes: is_array.then_some(size), bit_field: None });
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
                layout.fields.insert(name, StructField { member_type: Type::Struct { size: inner_size, align: inner_align as u8 }, offset: 0, struct_tag: Some(variant_tag), array_element: None, array_bytes: None, bit_field: None });
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
            let mut is_array = false;
            let mut size = type_size(field_type);
            if *self.peek() == Token::BracketOpen {
                is_array = true;
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
            layout.fields.insert(name, StructField { member_type: field_type, offset: 0, struct_tag, array_element, array_bytes: is_array.then_some(size), bit_field: None });
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
            let bump_before_item = self.skipped_inline_functions;
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
                // An uninitialized tentative definition (`int **g;` — a multi-level pointer the
                // scalar-only `Pointee` cannot represent) still emits a `.bss`/`.sbss` symbol in
                // mwcc; skipping it would silently drop that symbol (a whole-object DIFF), so defer.
                if self.item_is_uninitialized_definition() {
                    return Err(error);
                }
                // A skipped `static inline` function with an inline `asm {}` body
                // still contributes a local undefined symbol (mwcc cannot inline it).
                if let Some(name) = self.inline_asm_function_name() {
                    self.inline_asm_symbols.push(name);
                }
                // A skipped inline function's `static` locals (measured matrix):
                // a PLAIN inline emits each as a WEAK object named
                // `<local>$localstatic<K>$<function>` (K from 3, statics only,
                // per function; const -> .sdata2, non-zero -> .sdata, zero ->
                // .sbss), laid ahead of the pool constants, with NO @N shift.
                // A STATIC inline emits NO data but bumps the @N counter by 1
                // per static local. A CALL to either defers (the
                // skipped_inline_names check) — the called materialization is
                // unmodeled.
                if self.inline_function_has_static_local() {
                    let (function_name, is_static_inline, statics) = self.parse_skipped_inline_statics()?;
                    if is_static_inline {
                        // Positional numbering: sample the running bump BEFORE this
                        // inline's own counts apply — the static declares inside it.
                        for local in &statics {
                            self.static_local_prebumps.insert(local.name.clone(), self.skipped_inline_functions);
                        }
                        self.skipped_inline_functions += statics.len();
                    } else {
                        for (slot, local) in statics.into_iter().enumerate() {
                            let mangled = format!("{}$localstatic{}${}", local.name, slot + 3, function_name);
                            self.global_sizes.insert(mangled.clone(), (local.byte_size as u32, None));
                            globals.push(GlobalDeclaration {
                                non_static_functions_before: functions.iter().filter(|function| !function.is_static).count(),
                                declared_type: local.declared_type,
                                name: mangled,
                                is_extern: false,
                                is_static: false,
                                array_length: None,
                                initializer: None,
                                is_const: local.is_const,
                                address_initializer: None,
                                data_bytes: local.bytes,
                                data_relocations: Vec::new(),
                                is_weak: true,
                                section: None,
                            });
                        }
                    }
                }
                // A skipped INLINE function definition still advances mwcc's `@N`
                // counter by the labels its (compiled, then dropped) body uses.
                if let Some(bump) = self.skipped_inline_label_bump()? {
                    if std::env::var_os("MWCC_CAPTURE_DEBUG").is_some() {
                        eprintln!(
                            "inline-bump: {} +{bump} (total {})",
                            self.skipped_function_name().unwrap_or_default(),
                            self.skipped_inline_functions + bump
                        );
                    }
                    self.skipped_inline_functions += bump;
                    // A SINGLE-RETURN body is recorded for call-site
                    // substitution (mwcc -inline auto inlines it); anything
                    // else keeps only the NAME — a later call to it defers
                    // (a bl to the undefined local would be wrong bytes).
                    self.try_record_inline_body();
                    if let Some(name) = self.skipped_function_name() {
                        self.skipped_inline_names.insert(name);
                    }
                }
                // A skipped `typedef` still registers its alias name, so function
                // bodies that use the type as a pointer (`FILE *fp`) still parse.
                self.capture_skipped_typedef();
                self.skip_top_level_declaration();
            }
            if functions.len() > functions_before {
                seen_function = true;
                // A real function's own static locals number positionally too:
                // its body cannot add top-level inline definitions, so the bump
                // at the definition covers every declaration inside it.
                for function in &functions[functions_before..] {
                    for local in function.locals.iter().filter(|local| local.is_static) {
                        self.static_local_prebumps.insert(local.name.clone(), bump_before_item);
                    }
                }
            }
            // An emittable (non-`extern`, non-`const`) `static` global declared after
            // a function would need its local symbol interleaved among the functions'
            // `@N` entries — not yet modeled, so defer the unit honestly. A DEFINED
            // non-static global after a function needs the same source-order
            // interleaving in the global symbol run (mwcc: __upper_map AFTER
            // tolower in the MSL ctype shape) — also deferred until the writer
            // models it.
            if seen_function && globals[globals_before..].iter().any(|global| global.is_static && !global.is_const && !global.is_extern && global.section.is_none()) {
                return Err(Diagnostic::error("a static global declared after a function is not supported yet (local-symbol ordering)"));
            }

        }
        Ok(TranslationUnit {
            globals,
            functions,
            prototypes,
            inline_asm_symbols: std::mem::take(&mut self.inline_asm_symbols),
            skipped_inline_functions: self.skipped_inline_functions,
            static_local_prebumps: std::mem::take(&mut self.static_local_prebumps),
            implicitly_materialized: std::mem::take(&mut self.implicitly_materialized),
            weak_materialized: std::mem::take(&mut self.weak_materialized),
            skipped_inline_names: std::mem::take(&mut self.skipped_inline_names),
            deferred_function_names: std::mem::take(&mut self.deferred_function_names),
        })
    }

    /// If the item at the cursor is an `inline`/`static inline` function whose body
    /// contains an inline `asm` block, return its name (mwcc emits a local symbol
    /// for it). Pure lookahead — consumes nothing.
    /// Try to parse the inline definition at the cursor as
    /// `inline T name(T a, ...) { return expr; }` and record its body for
    /// call-site substitution. Restores the cursor either way.
    fn try_record_inline_body(&mut self) {
        let saved = self.position;
        let recorded = (|| -> Option<(String, Vec<String>, Expression)> {
            while matches!(self.peek(), Token::Identifier(word) if word == "static" || word == "inline" || word == "__inline") {
                self.advance();
            }
            self.parse_type().ok()?;
            let name = match self.advance().clone() {
                Token::Identifier(name) => name,
                _ => return None,
            };
            if *self.peek() != Token::ParenOpen {
                return None;
            }
            self.advance();
            let mut parameters = Vec::new();
            if *self.peek() == Token::KeywordVoid && self.tokens.get(self.position + 1) == Some(&Token::ParenClose) {
                self.advance();
            } else if *self.peek() != Token::ParenClose {
                loop {
                    self.parse_type().ok()?;
                    match self.advance().clone() {
                        Token::Identifier(parameter) => parameters.push(parameter),
                        _ => return None,
                    }
                    if *self.peek() == Token::Comma {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
            if *self.peek() != Token::ParenClose {
                return None;
            }
            self.advance();
            if *self.peek() != Token::BraceOpen {
                return None;
            }
            self.advance();
            if *self.peek() != Token::KeywordReturn {
                return None;
            }
            self.advance();
            let body = self.expression().ok()?;
            if *self.peek() != Token::Semicolon {
                return None;
            }
            self.advance();
            if *self.peek() != Token::BraceClose {
                return None;
            }
            Some((name, parameters, body))
        })();
        self.position = saved;
        if let Some((name, parameters, body)) = recorded {
            self.inline_bodies.insert(name, (parameters, body));
        }
    }

    /// The name of the (inline) function definition at the cursor: the last
    /// identifier before the parameter list's `(`.
    fn skipped_function_name(&self) -> Option<String> {
        let mut index = self.position;
        let mut name: Option<String> = None;
        while let Some(token) = self.tokens.get(index) {
            match token {
                Token::Identifier(word)
                    if word != "inline" && word != "__inline" && word != "static" && word != "extern" =>
                {
                    name = Some(word.clone());
                }
                Token::ParenOpen => return name,
                Token::Semicolon | Token::BraceOpen | Token::EndOfFile => return None,
                _ => {}
            }
            index += 1;
        }
        None
    }

    /// A braced aggregate initializer: `{ e, e, { ... }, "s" }` — elements are
    /// expressions, nested braces recurse. Parsed for AST fidelity; codegen
    /// defers on aggregate-initialized locals unless a capture claims the fn.
    fn aggregate_literal(&mut self) -> Compilation<Expression> {
        self.expect(Token::BraceOpen)?;
        let mut elements = Vec::new();
        while *self.peek() != Token::BraceClose {
            if *self.peek() == Token::BraceOpen {
                elements.push(self.aggregate_literal()?);
            } else {
                elements.push(self.expression()?);
            }
            if !self.eat_keyword(Token::Comma) {
                break;
            }
        }
        self.expect(Token::BraceClose)?;
        Ok(Expression::AggregateLiteral(elements))
    }

    fn inline_asm_function_name(&self) -> Option<String> {
        let mut index = self.position;
        let mut is_inline = false;
        let mut is_static = false;
        let mut name: Option<String> = None;
        // Signature up to the first `(`: note `static`/`inline`, and the last
        // identifier before the `(` (the function name).
        while let Some(token) = self.tokens.get(index) {
            match token {
                Token::Identifier(word) if word == "inline" || word == "__inline" => is_inline = true,
                Token::Identifier(word) if word == "static" => is_static = true,
                Token::Identifier(word) => name = Some(word.clone()),
                Token::ParenOpen => break,
                Token::Semicolon | Token::BraceOpen | Token::EndOfFile => return None,
                _ => {}
            }
            index += 1;
        }
        // Only a STATIC inline asm helper becomes the early local-UND symbol
        // (the measured OSFastCast.h shape). A PLAIN inline one (strikers'
        // __frsqrte) is a normal external created by the dropped compilation —
        // captures declare it via phantom_externals.
        if !is_inline || !is_static {
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

    /// True if the item at the cursor is an `inline`/`static inline` function whose
    /// body declares a `static` local. mwcc emits that static's data (`.sdata2` for a
    /// `const` scalar, `.sdata`/`.sbss` otherwise) even though the inline body is never
    /// emitted out-of-line when the function is uncalled — every variant tested emits
    /// extra data beyond the baseline. We don't model function-scope static data yet,
    /// so the caller defers the unit rather than silently drop that data (a whole-object
    /// DIFF). Pure lookahead — consumes nothing.
    /// Parse the skipped inline definition's `static` locals: the function
    /// name, whether the inline itself is `static`, and each local's type,
    /// const-ness, and byte image (`None` bytes = zero-initialized .sbss).
    fn parse_skipped_inline_statics(&self) -> Compilation<(String, bool, Vec<SkippedStaticLocal>)> {
        let mut index = self.position;
        let mut is_static_inline = false;
        let mut name = String::new();
        while let Some(token) = self.tokens.get(index) {
            match token {
                Token::Identifier(word) if word == "static" => is_static_inline = true,
                Token::Identifier(word) if word == "inline" || word == "__inline" => {}
                Token::Identifier(word) => name = word.clone(),
                Token::ParenOpen => break,
                _ => {}
            }
            index += 1;
        }
        // The parameter list: under `#pragma cplusplus on` the function's
        // symbol MANGLES CodeWarrior-style — `name__F<codes>` (f float,
        // d double, i int, v void) — and the $localstatic parent uses the
        // mangled name (measured: sqrtf(float) -> sqrtf__Ff).
        let mut parens = 0i32;
        let mut param_codes = String::new();
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
                Token::KeywordFloat => param_codes.push('f'),
                Token::Identifier(word) if self.typedefs.get(word) == Some(&Type::Float) => param_codes.push('f'),
                Token::Identifier(word) if word == "double" => param_codes.push('d'),
                Token::Identifier(word) if self.typedefs.get(word) == Some(&Type::Double) => param_codes.push('d'),
                Token::KeywordInt => param_codes.push('i'),
                Token::Identifier(word) if self.typedefs.get(word) == Some(&Type::Int) => param_codes.push('i'),
                Token::KeywordVoid => param_codes.push('v'),
                Token::Star => {
                    return Err(Diagnostic::error("a pointer parameter in a mangled inline is not supported yet (roadmap)"));
                }
                _ => {}
            }
            index += 1;
        }
        if self.cplusplus {
            if param_codes.is_empty() {
                param_codes.push('v');
            }
            name = format!("{name}__F{param_codes}");
        }
        let mut statics = Vec::new();
        let mut braces = 0i32;
        while let Some(token) = self.tokens.get(index) {
            match token {
                Token::BraceOpen => braces += 1,
                Token::BraceClose => {
                    braces -= 1;
                    if braces == 0 {
                        break;
                    }
                }
                Token::Identifier(word) if word == "static" && braces >= 1 => {
                    index += 1;
                    let mut is_const = false;
                    while matches!(self.tokens.get(index), Some(Token::Identifier(word)) if word == "const" || word == "volatile") {
                        if matches!(self.tokens.get(index), Some(Token::Identifier(word)) if word == "const") {
                            is_const = true;
                        }
                        index += 1;
                    }
                    // The type: one keyword/typedef token (compound int forms defer).
                    let declared_type = match self.tokens.get(index) {
                        Some(Token::Identifier(word)) if word == "double" => Type::Double,
                        Some(Token::KeywordFloat) => Type::Float,
                        Some(Token::KeywordInt) => Type::Int,
                        Some(Token::Identifier(word)) if self.typedefs.get(word) == Some(&Type::Double) => Type::Double,
                        Some(Token::Identifier(word)) if self.typedefs.get(word) == Some(&Type::Float) => Type::Float,
                        Some(Token::Identifier(word)) if self.typedefs.get(word) == Some(&Type::Int) => Type::Int,
                        Some(Token::Identifier(word)) if self.typedefs.get(word) == Some(&Type::UnsignedInt) => Type::UnsignedInt,
                        _ => return Err(Diagnostic::error("a static local of this type in an inline function is not supported yet (roadmap)")),
                    };
                    index += 1;
                    let local_name = match self.tokens.get(index) {
                        Some(Token::Identifier(word)) => word.clone(),
                        _ => return Err(Diagnostic::error("a static local declarator in an inline function is not supported yet (roadmap)")),
                    };
                    index += 1;
                    let bytes = match self.tokens.get(index) {
                        Some(Token::Semicolon) => None,
                        Some(Token::Equals) => {
                            index += 1;
                            let mut negative = false;
                            if matches!(self.tokens.get(index), Some(Token::Minus)) {
                                negative = true;
                                index += 1;
                            }
                            let image = match (self.tokens.get(index), declared_type) {
                                (Some(Token::FloatLiteral(value)), Type::Double) => {
                                    let value = if negative { -*value } else { *value };
                                    Some(value.to_be_bytes().to_vec())
                                }
                                (Some(Token::FloatLiteral(value)), Type::Float) => {
                                    let value = if negative { -*value } else { *value };
                                    Some((value as f32).to_be_bytes().to_vec())
                                }
                                (Some(Token::IntegerLiteral(value)), Type::Double) => {
                                    let value = if negative { -*value } else { *value };
                                    Some((value as f64).to_be_bytes().to_vec())
                                }
                                (Some(Token::IntegerLiteral(value)), Type::Float) => {
                                    let value = if negative { -*value } else { *value };
                                    Some((value as f32).to_be_bytes().to_vec())
                                }
                                (Some(Token::IntegerLiteral(value)), Type::Int | Type::UnsignedInt) => {
                                    let value = if negative { -*value } else { *value };
                                    let all_zero = value == 0;
                                    if all_zero { None } else { Some((value as i32).to_be_bytes().to_vec()) }
                                }
                                _ => return Err(Diagnostic::error("a static local initializer in an inline function is not supported yet (roadmap)")),
                            };
                            index += 1;
                            if !matches!(self.tokens.get(index), Some(Token::Semicolon)) {
                                return Err(Diagnostic::error("a static local initializer in an inline function is not supported yet (roadmap)"));
                            }
                            image
                        }
                        _ => return Err(Diagnostic::error("a static local declarator in an inline function is not supported yet (roadmap)")),
                    };
                    let byte_size = match declared_type {
                        Type::Double => 8u16,
                        _ => 4,
                    };
                    statics.push(SkippedStaticLocal { name: local_name, declared_type, is_const, bytes, byte_size });
                    continue;
                }
                Token::EndOfFile => break,
                _ => {}
            }
            index += 1;
        }
        Ok((name, is_static_inline, statics))
    }

    fn inline_function_has_static_local(&self) -> bool {
        let mut index = self.position;
        let mut is_inline = false;
        // Signature up to the first `(`: note `inline` (an `extern`/`static` qualifier
        // may precede it). A `;`/`{`/EOF before the `(` means this is not a function.
        while let Some(token) = self.tokens.get(index) {
            match token {
                Token::Identifier(word) if word == "inline" || word == "__inline" => is_inline = true,
                Token::ParenOpen => break,
                Token::Semicolon | Token::BraceOpen | Token::EndOfFile => return false,
                _ => {}
            }
            index += 1;
        }
        if !is_inline {
            return false;
        }
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
                Token::EndOfFile => return false,
                _ => {}
            }
            index += 1;
        }
        // The body must be a `{...}` block; scan just this body for a `static` local
        // (brace-matching stops at the function's own close brace, so a later function's
        // statics are not misattributed). A `static` identifier token inside a function
        // body is only ever a static-local storage class.
        if self.tokens.get(index) != Some(&Token::BraceOpen) {
            return false;
        }
        let mut braces = 0;
        while let Some(token) = self.tokens.get(index) {
            match token {
                Token::BraceOpen => braces += 1,
                Token::BraceClose => {
                    braces -= 1;
                    if braces == 0 {
                        break;
                    }
                }
                Token::Identifier(word) if word == "static" => return true,
                Token::EndOfFile => break,
                _ => {}
            }
            index += 1;
        }
        false
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
            // Surfaced pragmas switch the LANGUAGE for following declarations
            // (`#pragma cplusplus on` mangles their symbol names); push/pop
            // scope the switch.
            while let Token::Pragma(directive) = self.peek() {
                match directive.as_str() {
                    "push" => self.cplusplus_stack.push(self.cplusplus),
                    "pop" => self.cplusplus = self.cplusplus_stack.pop().unwrap_or(false),
                    "cplusplus on" => self.cplusplus = true,
                    "cplusplus off" => self.cplusplus = false,
                    "defer_codegen on" => self.defer_codegen = true,
                    "defer_codegen off" => self.defer_codegen = false,
                    _ => {}
                }
                self.advance();
            }
            let mut is_extern = false;
            let mut is_static = false;
            let mut is_weak = false;
            let mut declspec_section: Option<String> = None;
            let mut is_inline = false;
            while let Token::Identifier(word) = self.peek() {
                match word.as_str() {
                    "extern" => is_extern = true,
                    "static" => is_static = true,
                    "inline" | "__inline" => is_inline = true,
                    // `__declspec(weak)` marks the declared symbol WEAK — on a
                    // prototype it applies to the later definition too.
                    "__declspec" => {
                        self.advance();
                        self.expect(Token::ParenOpen)?;
                        let mut depth = 1;
                        let mut weak_inside = false;
                        // `__declspec(section "…")` — the string literal immediately
                        // following the `section` keyword names the output section.
                        let mut saw_section_kw = false;
                        while depth > 0 {
                            match self.advance() {
                                Token::ParenOpen => depth += 1,
                                Token::ParenClose => depth -= 1,
                                Token::Identifier(inner) if inner == "weak" => weak_inside = true,
                                Token::Identifier(inner) if inner == "section" => saw_section_kw = true,
                                Token::StringLiteral(bytes) if saw_section_kw => {
                                    declspec_section = Some(String::from_utf8_lossy(&bytes).into_owned());
                                    saw_section_kw = false;
                                }
                                Token::EndOfFile => return Err(Diagnostic::error("unterminated __declspec")),
                                _ => {}
                            }
                        }
                        if weak_inside {
                            is_weak = true;
                        }
                        continue;
                    }
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
                // A BODYLESS `typedef struct Tag Alias;` (a forward typedef —
                // the layout arrives when `struct Tag { ... }` is defined) or
                // `typedef struct Tag* AliasPtr;` registers the alias->TAG map
                // directly; member lookups resolve through the tag at use time.
                let is_union_forward = matches!(self.peek(), Token::Identifier(word) if word == "union");
                if (*self.peek() == Token::KeywordStruct || is_union_forward)
                    && matches!(self.tokens.get(self.position + 1), Some(Token::Identifier(_)))
                    && matches!(
                        (self.tokens.get(self.position + 2), self.tokens.get(self.position + 3)),
                        (Some(Token::Identifier(_)), Some(Token::Semicolon))
                            | (Some(Token::Identifier(_)), Some(Token::Comma))
                            | (Some(Token::Star), Some(Token::Identifier(_)))
                    )
                {
                    self.advance(); // `struct` / `union`
                    let tag = self.parse_identifier()?;
                    // One or more declarators: `Alias`, `*AliasPtr`, comma-
                    // separated (`typedef struct _IO_FILE _IO_FILE, *P_IO_FILE;`).
                    loop {
                        let is_pointer = self.eat_keyword(Token::Star);
                        let alias = self.parse_identifier()?;
                        if is_pointer {
                            self.struct_pointer_typedefs.insert(alias, tag.clone());
                        } else {
                            self.struct_typedefs.insert(alias, tag.clone());
                        }
                        if !self.eat_keyword(Token::Comma) {
                            break;
                        }
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
                    globals.push(GlobalDeclaration { is_weak: false, non_static_functions_before: functions.iter().filter(|function| !function.is_static).count(), declared_type: struct_type, name, is_extern, is_static, array_length: None, initializer: None, is_const: false, address_initializer: None, data_bytes: None, data_relocations: Vec::new(), section: declspec_section.clone() });
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
            // A PARENTHESIZED function declarator — `size_t (strlen)(...)`, the
            // MSL macro-protection form — is transparent: splice the parens out
            // of the token stream and fall into the ordinary declarator path.
            if *self.peek() == Token::ParenOpen
                && matches!(self.peek_at(1), Token::Identifier(_))
                && *self.peek_at(2) == Token::ParenClose
            {
                self.tokens.remove(self.position); // `(`
                self.tokens.remove(self.position + 1); // `)` (the name shifted down)
            }
            // Function-pointer declarator: `RET (*name)(params)` — a pointer-typed
            // global (a 4-byte address). The return/parameter types don't affect
            // codegen, so the signature is skipped.
            if *self.peek() == Token::ParenOpen {
                self.advance();
                self.expect(Token::Star)?;
                let pointer_name = self.parse_identifier()?;
                // An ARRAY of function pointers: `void (*atexit_funcs[64])(void);`
                // — `[N]` (or `[]` on an extern reference) between the name and
                // the closing paren. Each element is a 4-byte address.
                let mut pointer_array_length: Option<u16> = None;
                if self.eat_keyword(Token::BracketOpen) {
                    if let Token::IntegerLiteral(count) = self.peek() {
                        pointer_array_length = Some(*count as u16);
                        self.advance();
                    }
                    self.expect(Token::BracketClose)?;
                    if pointer_array_length.is_none() && !is_extern {
                        return Err(Diagnostic::error("a function-pointer array needs an explicit length (roadmap)"));
                    }
                }
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
                // Optional initializer: `= 0` (a NULL pointer — an all-null address initializer,
                // which the object writer lands in `.sbss` as an EXPLICIT zero) or `= func` / `= &func`
                // (an ADDR32 relocation to that symbol in `.sdata`). Both flow through the same
                // address-initializer path the data-pointer globals use.
                let address_initializer = if self.eat_keyword(Token::Equals) {
                    Some(self.parse_address_initializer()?)
                } else {
                    None
                };
                self.expect(Token::Semicolon)?;
                globals.push(GlobalDeclaration { is_weak: false, non_static_functions_before: functions.iter().filter(|function| !function.is_static).count(), declared_type: Type::StructPointer { element_size: 0 }, name: pointer_name, is_extern, is_static, array_length: pointer_array_length, initializer: None, is_const: false, address_initializer, data_bytes: None, data_relocations: Vec::new(), section: declspec_section.clone() });
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
                if let Some(tag) = &global_struct_tag {
                    self.global_structs.insert(name.clone(), tag.clone());
                }
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
                    let mut data_relocations: Vec<(u32, String, i32)> = Vec::new();
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
                        let mut relocations = Vec::new();
                        data_bytes = Some(if dimensions.is_empty() {
                            self.parse_one_struct_relocated(&tag, 0, &mut relocations)?
                        } else {
                            self.parse_struct_array_initializer(&tag, &mut relocations)?
                        });
                        data_relocations = relocations;
                    } else if self.eat_keyword(Token::Equals) {
                        // `= <constant>` or `= { <constant>, ... }` (nested braces flatten).
                        initializer = Some(self.parse_constant_initializer(return_type)?);
                    } else if *self.peek() == Token::Colon {
                        // A MWERKS absolute-placement declaration `T name[dims] : <address>;`
                        // binds the name to a FIXED address (memory-mapped hardware registers —
                        // dolphin/hw_regs.h's `volatile u16 __VIRegs[59] : 0xCC002000;`). mwcc
                        // emits NO symbol or data for it (references resolve to the absolute
                        // address). We don't model those references yet, so skip the declaration
                        // entirely rather than emit it as a `.bss` object (a whole-object DIFF for
                        // every dolphin.h-including TU); a reference to the name then defers.
                        self.advance();
                        self.parse_integer_constant()?; // the absolute address
                        self.expect(Token::Semicolon)?;
                        return Ok(());
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
                    // mwcc INLINES a `const` scalar-int global's value at each read (`return g` ->
                    // `li r3,VALUE`) while still emitting g's read-only `.sdata2` storage. Fold reads
                    // like an enum constant; the global is still pushed below so the writer emits the
                    // storage. A narrow const reads as its value EXTENDED to int per its signedness
                    // (`const char c=200` reads -56; `const unsigned char=200` reads 200) while the
                    // storage keeps the raw byte — so fold the value reduced to the declared width.
                    // (extern has no initializer; `&g` then folds to AddressOf{literal} and defers —
                    // safe, not a wrong load.)
                    if is_const && !is_extern && dimensions.is_empty()
                        && matches!(return_type, Type::Int | Type::UnsignedInt | Type::Char | Type::UnsignedChar | Type::Short | Type::UnsignedShort)
                        && initializer.as_ref().map_or(false, |values| values.len() == 1)
                    {
                        let folded = crate::expressions::truncate_to_integer(initializer.as_ref().unwrap()[0], return_type);
                        self.enum_constants.insert(declarator_name.clone(), folded);
                    }
                    // Record the global's total byte size so `sizeof(g)` folds to a constant, plus its
                    // array element size (Some only for an array) so `sizeof(g[0])` folds too — the
                    // classic `sizeof(a)/sizeof(a[0])` element count.
                    let element_bytes = match return_type {
                        Type::Struct { size, .. } => size as u32,
                        Type::Pointer(_) | Type::StructPointer { .. } => 4,
                        other => other.width() as u32 / 8,
                    };
                    let total_bytes = element_bytes * array_length.map_or(1, u32::from);
                    let array_element = array_length.map(|_| element_bytes);
                    self.global_sizes.insert(declarator_name.clone(), (total_bytes, array_element));
                    // For a POINTER declarator, a LEADING `const` binds the
                    // POINTEE (`const char* dummy = "C"` is a WRITABLE pointer
                    // in `.sdata` — measured: locale) — the object itself is
                    // not const.
                    let object_is_const = is_const && !matches!(return_type, Type::Pointer(_) | Type::StructPointer { .. });
                    globals.push(GlobalDeclaration { is_weak: false, non_static_functions_before: functions.iter().filter(|function| !function.is_static).count(), declared_type: return_type, name: declarator_name, is_extern, is_static, array_length, initializer, is_const: object_is_const, address_initializer, data_bytes, data_relocations: std::mem::take(&mut data_relocations), section: declspec_section.clone() });
                    if *self.peek() == Token::Comma {
                        self.advance();
                        // A later pointer declarator carries its own `*` (`int *a, *b;`): the base type
                        // is already the pointer type formed by the first declarator, so consume the `*`
                        // and reuse it. A MIXED list (`int *a, b;`) or a MULTI-LEVEL one (`int *a, **b;`)
                        // needs a per-declarator type, so defer rather than mis-type a declarator.
                        if *self.peek() == Token::Star {
                            if !matches!(return_type, Type::Pointer(_) | Type::StructPointer { .. }) {
                                return Err(Diagnostic::error("a mixed pointer/non-pointer global declarator list is not supported yet (roadmap)"));
                            }
                            self.advance();
                            if *self.peek() == Token::Star {
                                return Err(Diagnostic::error("a multi-level pointer global declarator list is not supported yet (roadmap)"));
                            }
                        }
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
                if is_weak {
                    self.weak_functions.insert(name.clone());
                }
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
            // A `static inline` DEFINITION is normally skipped-and-inlined (the
            // mp4 shape — the error routes it to the skip machinery). But with a
            // PRIOR PROTOTYPE the call sites precede the body, so mwcc cannot
            // inline it: it MATERIALIZES out-of-line as a local function at the
            // definition's source position (measured: AC/ww/sunshine uart).
            if is_inline {
                // Referenced EARLIER (a prototype, or a call already parsed into a
                // previous function — uart_8's IMPLICIT-declaration shape) means the
                // call sites precede the body: mwcc cannot inline and MATERIALIZES.
                let name_set: std::collections::HashSet<String> = std::iter::once(name.clone()).collect();
                let had_prototype = prototypes.iter().any(|(prototype_name, _, _)| *prototype_name == name);
                let had_call = functions.iter().any(|earlier| {
                    earlier.statements.iter().any(|statement| statement_calls(statement, &name_set))
                        || earlier.guards.iter().any(|guard| expression_calls(&guard.condition, &name_set))
                        || earlier.return_expression.as_ref().is_some_and(|expression| expression_calls(expression, &name_set))
                });
                // The trigger is a CALL compiled before the definition — a
                // prototype alone does NOT materialize (p2's wctomb: prototyped,
                // defined, THEN called — mwcc inlines it at the later call).
                if !had_call {
                    return Err(Diagnostic::error("an inline function definition is skipped (inlined at call sites)"));
                }
                if is_static {
                    // Implicit-declaration materialization (no prototype): the call
                    // relocations bind the surviving UND ghost, and the local FUNC
                    // symbol trails its own static locals (measured: ww uart).
                    if !had_prototype {
                        self.implicitly_materialized.push(name.clone());
                    }
                } else {
                    // A PLAIN inline materializes as a WEAK global (measured:
                    // strikers mbstring's `inline int mbstowcs` — FUNC WEAK,
                    // with the weak-OBJECT 0x0d comment flag, not declspec's 0x0e).
                    is_weak = true;
                    self.weak_materialized.push(name.clone());
                }
            }
            let function_is_weak = is_weak || self.weak_functions.contains(&name);
            if self.defer_codegen {
                self.deferred_function_names.push(name.clone());
            }
            let mut function = self.function_body(return_type, name, is_static, parameters)?;
            function.is_weak = function_is_weak;
            functions.push(function);
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

    /// Whether the item at the cursor is an uninitialized (tentative) scalar data *definition* — a
    /// non-`extern` `<scalar type> <name>[…];` with no initializer and no function parentheses
    /// (e.g. `int **g;`, whose `int **` type the scalar-only `Pointee` cannot represent). mwcc emits
    /// a `.bss`/`.sbss`/`.comm` symbol for such a tentative definition, so SKIPPING it on a parse
    /// failure would drop the symbol — a silent whole-object DIFF. Defer instead. Pure lookahead.
    fn item_is_uninitialized_definition(&self) -> bool {
        // Must start with a scalar type keyword: a struct/union/enum, a typedef alias, or an
        // `extern`-led declaration emits no tentative data symbol, so those stay skippable.
        if !matches!(
            self.tokens.get(self.position),
            Some(Token::KeywordInt | Token::KeywordChar | Token::KeywordShort | Token::KeywordUnsigned | Token::KeywordFloat | Token::KeywordVoid)
        ) {
            return false;
        }
        // A top-level `(` (function/prototype), `=` (initialized — the other detector handles it),
        // or `{` (a body) means it is not a bare tentative definition; a `;`/`,` after a name is.
        let mut index = self.position;
        let mut saw_name = false;
        while let Some(token) = self.tokens.get(index) {
            match token {
                Token::ParenOpen | Token::Equals | Token::BraceOpen => return false,
                Token::Identifier(_) => saw_name = true,
                Token::Semicolon | Token::Comma => return saw_name,
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
    /// Like `item_is_function_definition`, but for the `inline`/`__inline`
    /// definitions that check deliberately skips.
    /// If the item at the cursor is a skipped INLINE function definition,
    /// the @N labels mwcc consumes compiling (then dropping) it — measured per
    /// construct: a STATIC definition has base 3, a plain one 0; each `if`
    /// adds 2; `else`/`switch`/`case`/`default`/`||`/`&&` add 1; `while` adds
    /// 4, `for` 5; a ternary adds 0. Unmeasured control constructs (`do`,
    /// `goto`) return an Err so the unit defers rather than mis-bump.
    fn skipped_inline_label_bump(&self) -> Compilation<Option<usize>> {
        let mut index = self.position;
        let mut paren_depth = 0i32;
        let mut saw_parameter_list = false;
        let mut saw_inline = false;
        let mut saw_static = false;
        while let Some(token) = self.tokens.get(index) {
            match token {
                Token::Identifier(word) if word == "typedef" => return Ok(None),
                Token::Identifier(word) if word == "static" => saw_static = true,
                Token::Identifier(word) if word == "inline" || word == "__inline" => saw_inline = true,
                Token::ParenOpen => paren_depth += 1,
                Token::ParenClose => {
                    paren_depth -= 1;
                    if paren_depth == 0 {
                        saw_parameter_list = true;
                    }
                }
                Token::Semicolon if paren_depth == 0 => return Ok(None),
                Token::BraceOpen if paren_depth == 0 => {
                    if !(saw_inline && saw_parameter_list) {
                        return Ok(None);
                    }
                    // Scan the body braces, summing the measured label weights.
                    let mut bump = if saw_static { 3usize } else { 0 };
                    let mut brace_depth = 0i32;
                    // `&&`/`||` count ONLY inside a CONDITION's parens (fire 493:
                    // value-position short-circuits add nothing).
                    let mut condition_pending = false;
                    let mut condition_depth = 0i32;
                    while let Some(token) = self.tokens.get(index) {
                        match token {
                            Token::ParenOpen => {
                                if condition_pending || condition_depth > 0 {
                                    condition_depth += 1;
                                    condition_pending = false;
                                }
                            }
                            Token::ParenClose => {
                                if condition_depth > 0 {
                                    condition_depth -= 1;
                                }
                            }
                            Token::BraceOpen => brace_depth += 1,
                            Token::BraceClose => {
                                brace_depth -= 1;
                                if brace_depth == 0 {
                                    return Ok(Some(bump));
                                }
                            }
                            Token::KeywordIf => {
                                bump += 2;
                                condition_pending = true;
                            }
                            Token::Identifier(word) if word == "else" => bump += 1,
                            Token::Identifier(word) if word == "switch" => bump += 1,
                            Token::Identifier(word) if word == "case" => bump += 1,
                            Token::Identifier(word) if word == "default" => bump += 1,
                            Token::PipePipe | Token::AmpersandAmpersand if condition_depth > 0 => bump += 1,
                            Token::KeywordWhile => {
                                bump += 4;
                                condition_pending = true;
                            }
                            Token::KeywordFor => {
                                bump += 5;
                                condition_pending = true;
                            }
                            // A do-while contributes +4 TOTAL (measured fire 493)
                            // — its `while` token below carries the count, so the
                            // `do` itself is transparent.
                            Token::KeywordDo => {}
                            Token::Identifier(word) if word == "goto" => bump += 1, // measured: goto+label = +1
                            Token::EndOfFile => return Ok(None),
                            _ => {}
                        }
                        index += 1;
                    }
                    return Ok(None);
                }
                Token::EndOfFile => return Ok(None),
                _ => {}
            }
            index += 1;
        }
        Ok(None)
    }

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
    /// Whether the `{` at the cursor closes immediately before the function's
    /// own closing brace — i.e. it wraps the WHOLE remaining body.
    fn brace_wraps_whole_body(&self) -> bool {
        let mut index = self.position;
        let mut depth = 0i32;
        while let Some(token) = self.tokens.get(index) {
            match token {
                Token::BraceOpen => depth += 1,
                Token::BraceClose => {
                    depth -= 1;
                    if depth == 0 {
                        return self.tokens.get(index + 1) == Some(&Token::BraceClose);
                    }
                }
                Token::EndOfFile => return false,
                _ => {}
            }
            index += 1;
        }
        false
    }

    fn function_body(&mut self, return_type: Type, name: String, is_static: bool, parameters: Vec<Parameter>) -> Compilation<Function> {
        self.expect(Token::BraceOpen)?;
        // A redundant WHOLE-BODY block `int f() { { ... } }` (a macro
        // artifact — the MSL ctype shape) is transparent: consume the inner
        // brace; its matching close is consumed with the function's own.
        let mut redundant_blocks = 0usize;
        while *self.peek() == Token::BraceOpen && self.brace_wraps_whole_body() {
            self.advance();
            redundant_blocks += 1;
        }

        // Track each parameter's type (function-scoped — cleared per function) so `sizeof(param)`
        // folds to a `size_t` constant.
        self.variable_types.clear();
        self.variable_array_bytes.clear();
        for parameter in &parameters {
            self.variable_types.insert(parameter.name.clone(), parameter.parameter_type);
        }

        // Zero or more local declarations precede the return statement. A
        // statement that begins with a type keyword is a local declaration;
        // `return` ends the body.
        let mut locals = Vec::new();
        // A local declaration may open with a storage-class keyword: `static` gives the variable
        // static storage (codegen'd like a global, so recorded and deferred for now), while
        // `register`/`auto` are ordinary-automatic hints with no codegen effect. These are
        // `Identifier` tokens, so peek past them before the type test below.
        loop {
            let mut is_static = false;
            while let Token::Identifier(word) = self.peek() {
                match word.as_str() {
                    "static" => is_static = true,
                    "register" | "auto" => {}
                    _ => break,
                }
                self.advance();
            }
            if !self.peek_is_type() {
                break;
            }
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
                // `RET (*name)(params)` / `RET (**name)(params)` — a function-
                // pointer (or pointer to one) LOCAL: a 4-byte word; the signature
                // is skipped (abort_exit's `void (**var_r31)(void);`).
                if *self.peek() == Token::ParenOpen && self.tokens.get(self.position + 1) == Some(&Token::Star) {
                    self.advance(); // `(`
                    self.advance(); // `*`
                    self.eat_keyword(Token::Star);
                    let name = self.parse_identifier()?;
                    self.expect(Token::ParenClose)?;
                    self.expect(Token::ParenOpen)?;
                    let mut depth = 1;
                    while depth > 0 {
                        match self.advance() {
                            Token::ParenOpen => depth += 1,
                            Token::ParenClose => depth -= 1,
                            Token::EndOfFile => return Err(Diagnostic::error("unterminated function-pointer local")),
                            _ => {}
                        }
                    }
                    let initializer = if self.eat_keyword(Token::Equals) { Some(self.expression()?) } else { None };
                    locals.push(LocalDeclaration { declared_type: Type::Pointer(Pointee::Pointer), name, initializer, array_length: None, is_static: false, data_bytes: None, is_const: false });
                    if self.eat_keyword(Token::Comma) {
                        continue;
                    }
                    // The shared tail after the declarator loop consumes the `;`.
                    break;
                }
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
                // A local array `type buf[N];` — a frame slot of `N` elements. A
                // STATIC local array (`static const f32 c[] = {...};`) captures its
                // byte image instead (it is static storage, not a frame slot).
                let mut data_relocations: Vec<(u32, String, i32)> = Vec::new();
                    let mut data_bytes: Option<Vec<u8>> = None;
                let array_length = if *self.peek() == Token::BracketOpen {
                    self.advance();
                    let explicit = if *self.peek() == Token::BracketClose {
                        None
                    } else {
                        Some(self.parse_integer_constant()? as u16)
                    };
                    self.expect(Token::BracketClose)?;
                    if *self.peek() == Token::BracketOpen {
                        return Err(Diagnostic::error("a multi-dimensional local array is not supported yet (roadmap)"));
                    }
                    if *self.peek() == Token::Equals {
                        // An AUTOMATIC initialized array parses like the static
                        // form (its byte image on the local); NATIVE codegen for
                        // the frame copy-in is unmodeled, so the GENERATOR defers
                        // it AFTER the exact-match templates get a claim.
                        self.advance();
                        self.expect(Token::BraceOpen)?;
                        let mut bytes = Vec::new();
                        let mut count = 0u16;
                        loop {
                            if *self.peek() == Token::BraceClose {
                                break;
                            }
                            let mut negative = false;
                            if self.eat_keyword(Token::Minus) {
                                negative = true;
                            }
                            match (self.advance().clone(), declared_type) {
                                (Token::FloatLiteral(value), Type::Float) => {
                                    let value = if negative { -value } else { value };
                                    bytes.extend_from_slice(&(value as f32).to_be_bytes());
                                }
                                (Token::FloatLiteral(value), Type::Double) => {
                                    let value = if negative { -value } else { value };
                                    bytes.extend_from_slice(&value.to_be_bytes());
                                }
                                (Token::IntegerLiteral(value), Type::Float) => {
                                    let value = if negative { -value } else { value };
                                    bytes.extend_from_slice(&(value as f32).to_be_bytes());
                                }
                                (Token::IntegerLiteral(value), Type::Double) => {
                                    let value = if negative { -value } else { value };
                                    bytes.extend_from_slice(&(value as f64).to_be_bytes());
                                }
                                (Token::IntegerLiteral(value), Type::Int | Type::UnsignedInt) => {
                                    let value = if negative { -value } else { value };
                                    bytes.extend_from_slice(&(value as i32).to_be_bytes());
                                }
                                (Token::IntegerLiteral(value), Type::Char | Type::UnsignedChar) => {
                                    let value = if negative { -value } else { value };
                                    bytes.push(value as u8);
                                }
                                (Token::IntegerLiteral(value), Type::Short | Type::UnsignedShort) => {
                                    let value = if negative { -value } else { value };
                                    bytes.extend_from_slice(&(value as i16).to_be_bytes());
                                }
                                _ => return Err(Diagnostic::error("a static local array initializer element is not supported yet (roadmap)")),
                            }
                            count += 1;
                            if !self.eat_keyword(Token::Comma) {
                                break;
                            }
                        }
                        self.expect(Token::BraceClose)?;
                        data_bytes = Some(bytes);
                        Some(explicit.unwrap_or(count))
                    } else {
                        match explicit {
                            Some(length) => Some(length),
                            None => return Err(Diagnostic::error("an array with no length needs an initializer")),
                        }
                    }
                } else {
                    None
                };
                let initializer = if array_length.is_none() && self.eat_keyword(Token::Equals) {
                    if *self.peek() == Token::BraceOpen {
                        Some(self.aggregate_literal()?)
                    } else {
                        Some(self.expression()?)
                    }
                } else {
                    None
                };
                // A scalar local's type — and an array's ELEMENT type — feeds `sizeof(local)` and
                // `sizeof(local[i])`/`sizeof(*local)`; an array also records its TOTAL byte size
                // (element size * length) for `sizeof(arr)`.
                self.variable_types.insert(name.clone(), declared_type);
                if let Some(length) = array_length {
                    let element_bytes = match declared_type {
                        Type::Struct { size, .. } => size as u32,
                        Type::Pointer(_) | Type::StructPointer { .. } => 4,
                        other => other.width() as u32 / 8,
                    };
                    self.variable_array_bytes.insert(name.clone(), element_bytes * length as u32);
                }
                locals.push(LocalDeclaration { declared_type, name, initializer, array_length, is_static, data_bytes, is_const: self.last_type_was_const });
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
        let mut local_names: std::collections::HashSet<String> = locals.iter().map(|local| local.name.clone()).collect();
        local_names.extend(parameters.iter().map(|parameter| parameter.name.clone()));
        // Block-scoped declarations hoist here (their initializations stay as
        // positioned Assign statements inside their blocks).
        let mut block_locals: Vec<LocalDeclaration> = Vec::new();
        let mut statements = Vec::new();
        // Zero or more guarded early returns: `if (condition) return value;`. An
        // `if (c) return x; else return y;` terminates the function as a single
        // conditional return (the ternary `c ? x : y`).
        let mut guards: Vec<GuardedReturn> = Vec::new();
        let mut conditional_return = None;
        'body: loop {
            while *self.peek() != Token::BraceClose {
                // A `return` mid-body: TERMINAL (the function's trailing return —
                // its `;` is directly followed by `}`) exits to the guard/return
                // machinery below; a NON-terminal one (a goto label or further
                // statements follow, the string.c shape) is a positioned
                // Statement::Return in the ordered list.
                if *self.peek() == Token::KeywordReturn {
                    if self.return_is_terminal() {
                        break;
                    }
                    statements.push(self.parse_return_statement()?);
                    continue;
                }
                // A bare `{ ... }` scoping block is TRANSPARENT: its statements
                // flatten into the enclosing list and its declarations hoist
                // like other block-scoped locals (strtold's exponent block).
                if *self.peek() == Token::BraceOpen {
                    let mut inner = self.parse_block(&mut local_names, &mut block_locals)?;
                    statements.append(&mut inner);
                    continue;
                }
                // An empty statement (a lone `;`) produces no code — skip it.
                if *self.peek() == Token::Semicolon {
                    self.advance();
                    continue;
                }
                // `if (c) { ... }` is a conditional block statement; a trailing
                // `if (c) return ...` is a guard, handled after the statement list.
                if *self.peek() == Token::KeywordIf {
                    if self.block_if_ahead() {
                        let statement = self.parse_if_statement(&mut local_names, &mut block_locals)?;
                        statements.push(statement);
                        continue;
                    }
                    break;
                }
                if matches!(self.peek(), Token::KeywordWhile | Token::KeywordDo | Token::KeywordFor) {
                    statements.push(self.parse_loop_statement(&mut local_names, &mut block_locals)?);
                    continue;
                }
                if let Some(statement) = self.parse_jump_statement()? {
                    statements.push(statement);
                    continue;
                }
                let statement = self.parse_simple_statement(&mut local_names, &mut block_locals)?;
                statements.push(statement);
            }

            while *self.peek() == Token::KeywordIf {
                // A block-if here follows the guards, so the body CONTINUES — it is
                // parsed by the statement loop after the migration below.
                if self.block_if_ahead() {
                    break;
                }
                self.advance();
                self.expect(Token::ParenOpen)?;
                let condition = self.expression()?;
                self.expect(Token::ParenClose)?;
                let Some(value) = self.parse_guard_return()? else {
                    // A bare `if (c) return;` (a void early return) has no guard value —
                    // migrate the pending guards and this if into the ordered statement
                    // list (as the continuation migration below does) and resume the
                    // statement loop for whatever follows.
                    for guard in guards.drain(..) {
                        statements.push(Statement::If {
                            condition: guard.condition,
                            then_body: vec![Statement::Return(Some(guard.value))],
                            else_body: Vec::new(),
                        });
                    }
                    statements.push(Statement::If {
                        condition,
                        then_body: vec![Statement::Return(None)],
                        else_body: Vec::new(),
                    });
                    continue 'body;
                };
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
                    // A NON-RETURN else body (`if (c1) return v1; else if (c2) return v2;
                    // else { n = …; … }` — the fdlibm trig-dispatch shape): every prior
                    // branch returns, so the else block is simply the CONTINUING body.
                    // Migrate the pending guards and this one into the ordered statement
                    // list, splice the else body's statements, and resume the statement
                    // loop for whatever follows the block.
                    let else_returns = *self.peek() == Token::KeywordReturn
                        || (*self.peek() == Token::BraceOpen && *self.peek_at(1) == Token::KeywordReturn);
                    if !else_returns {
                        for guard in guards.drain(..) {
                            statements.push(Statement::If {
                                condition: guard.condition,
                                then_body: vec![Statement::Return(Some(guard.value))],
                                else_body: Vec::new(),
                            });
                        }
                        statements.push(Statement::If {
                            condition,
                            then_body: vec![Statement::Return(Some(value))],
                            else_body: Vec::new(),
                        });
                        statements.extend(self.parse_block_or_statement(&mut local_names, &mut block_locals)?);
                        continue 'body;
                    }
                    // `if (c) return v; else return d;` is the guard `if (c) return v;`
                    // with fall-through `d` — routed through the guard codegen (which
                    // normalizes a negated `!c` to keep `v` as the in-place default, as
                    // mwcc does) rather than emitted as a bare `(c) ? v : d` ternary.
                    let Some(otherwise) = self.parse_guard_return()? else {
                        return Err(Diagnostic::error("a bare `return;` in an else branch is not supported yet (roadmap)"));
                    };
                    // The body CONTINUES past the full-return diamond (a goto
                    // label follows — melee string.c's `adjust:`): migrate the
                    // pending guards and this if/else into the ordered list and
                    // resume the statement loop.
                    if *self.peek() != Token::BraceClose {
                        for guard in guards.drain(..) {
                            statements.push(Statement::If {
                                condition: guard.condition,
                                then_body: vec![Statement::Return(Some(guard.value))],
                                else_body: Vec::new(),
                            });
                        }
                        statements.push(Statement::If {
                            condition,
                            then_body: vec![Statement::Return(Some(value))],
                            else_body: vec![Statement::Return(Some(otherwise))],
                        });
                        continue 'body;
                    }
                    guards.push(GuardedReturn { condition, value });
                    conditional_return = Some(otherwise);
                    break;
                }
                guards.push(GuardedReturn { condition, value });
            }

            // Trailing guards end the body at the final return or the closing brace.
            // A NON-terminal return (a goto label follows) instead migrates the
            // guards below and resumes the statement loop, which records it as a
            // positioned Statement::Return.
            if *self.peek() == Token::BraceClose
                || conditional_return.is_some()
                || (*self.peek() == Token::KeywordReturn && self.return_is_terminal())
            {
                break;
            }
            // The body CONTINUES past the guards (`if (c) return -1; x = …;`): the flat
            // statements→guards split cannot hold that order, so migrate the pending
            // guards into the ordered statement list as early-return ifs and resume the
            // statement loop. Source order is preserved — later trailing guards still
            // follow every statement. The general-control-flow codegen defers these
            // bodies (emit_statement rejects If/Return), so this never emits wrong bytes.
            for guard in guards.drain(..) {
                statements.push(Statement::If {
                    condition: guard.condition,
                    then_body: vec![Statement::Return(Some(guard.value))],
                    else_body: Vec::new(),
                });
            }
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
        // Stray empty statements (`;`) may trail the return before the closing brace
        // (`return x;;` or a lone `;`) — they produce no code, so skip them.
        while *self.peek() == Token::Semicolon {
            self.advance();
        }
        self.expect(Token::BraceClose)?;
        for _ in 0..redundant_blocks {
            self.expect(Token::BraceClose)?;
        }

        let mut locals = locals;
        locals.extend(block_locals);
        Ok(Function { return_type, name, is_static, is_weak: false, parameters, locals, statements, guards, return_expression })
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
    /// Whether the `return` at the cursor is the function's TRAILING return:
    /// its statement-ending `;` is directly followed by the closing `}`. A
    /// return expression never contains a semicolon, so the first `;` ahead
    /// ends the statement.
    fn return_is_terminal(&self) -> bool {
        let mut offset = 1;
        loop {
            match self.peek_at(offset) {
                Token::Semicolon => break,
                Token::EndOfFile => return true,
                _ => offset += 1,
            }
        }
        // Stray `;;` after the return still ends the body — skip empties.
        let mut offset = offset + 1;
        while *self.peek_at(offset) == Token::Semicolon {
            offset += 1;
        }
        *self.peek_at(offset) == Token::BraceClose
    }

    /// A jump statement or label in statement position: `break;`, `continue;`,
    /// `goto name;`, or `name:` (an identifier directly followed by a colon —
    /// never a valid expression statement, so the lookahead is unambiguous).
    /// Returns None when the next tokens are none of these.
    fn parse_jump_statement(&mut self) -> Compilation<Option<Statement>> {
        let Token::Identifier(word) = self.peek() else {
            return Ok(None);
        };
        match word.as_str() {
            "break" => {
                self.advance();
                self.expect(Token::Semicolon)?;
                Ok(Some(Statement::Break))
            }
            "continue" => {
                self.advance();
                self.expect(Token::Semicolon)?;
                Ok(Some(Statement::Continue))
            }
            "goto" => {
                self.advance();
                let name = self.parse_identifier()?;
                self.expect(Token::Semicolon)?;
                Ok(Some(Statement::Goto(name)))
            }
            _ if *self.peek_at(1) == Token::Colon => {
                let name = self.parse_identifier()?;
                self.advance(); // the colon
                Ok(Some(Statement::Label(name)))
            }
            _ => Ok(None),
        }
    }

    fn parse_simple_statement(&mut self, local_names: &mut std::collections::HashSet<String>, block_locals: &mut Vec<LocalDeclaration>) -> Compilation<Statement> {
        if matches!(self.peek(), Token::Identifier(word) if word == "switch") {
            return self.parse_switch(local_names, block_locals);
        }
        let first = self.factor()?;
        // Prefix `++`/`--` desugars to `target = target ± 1` in factor; the
        // POSTFIX form arrives as PostStep and lowers here, where the value
        // is discarded (the two forms coincide only in that case).
        let first = lower_discarded_post_step(first);
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
        } else if *self.peek() == Token::Semicolon {
            self.advance();
            Ok(Statement::Expression(first))
        } else {
            // A discarded BINARY expression statement (`t & w;` — dead code in
            // MSL string.c): parse the full expression for a faithful AST; the
            // pure discarded form has no lowering yet, so codegen defers.
            let expression = self.binary_expression_from(first, 1)?;
            self.expect(Token::Semicolon)?;
            Ok(Statement::Expression(expression))
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
    /// guard codegen is identical either way. A bare `return;` (a void early return)
    /// yields `None` — it cannot become a `GuardedReturn` (whose value is required),
    /// so the caller routes it into the ordered statement list instead.
    fn parse_guard_return(&mut self) -> Compilation<Option<Expression>> {
        let braced = self.eat_keyword(Token::BraceOpen);
        self.expect(Token::KeywordReturn)?;
        if *self.peek() == Token::Semicolon {
            self.advance();
            if braced {
                self.expect(Token::BraceClose)?;
            }
            return Ok(None);
        }
        let value = self.expression()?;
        self.expect(Token::Semicolon)?;
        if braced {
            self.expect(Token::BraceClose)?;
        }
        Ok(Some(value))
    }

    /// `return [value];` as a body statement (an early return), with an optional
    /// value (absent for `return;` in a void function).
    fn parse_return_statement(&mut self) -> Compilation<Statement> {
        self.expect(Token::KeywordReturn)?;
        let value = if *self.peek() == Token::Semicolon { None } else { Some(self.expression()?) };
        self.expect(Token::Semicolon)?;
        Ok(Statement::Return(value))
    }

    /// A condition expression that may use the top-level comma operator
    /// (`if ((a = x), test)` — alloc.c's link/merge macros). Each left
    /// operand runs for side effects; the last operand is the value.
    fn parse_comma_expression(&mut self) -> Compilation<Expression> {
        let mut expression = self.expression()?;
        while *self.peek() == Token::Comma {
            self.advance();
            let right = self.expression()?;
            expression = Expression::Comma { left: Box::new(expression), right: Box::new(right) };
        }
        Ok(expression)
    }

    /// `if (condition) <block-or-statement> [else <block-or-statement> | else if]`.
    fn parse_if_statement(&mut self, local_names: &mut std::collections::HashSet<String>, block_locals: &mut Vec<LocalDeclaration>) -> Compilation<Statement> {
        self.expect(Token::KeywordIf)?;
        self.expect(Token::ParenOpen)?;
        let condition = self.parse_comma_expression()?;
        self.expect(Token::ParenClose)?;
        let then_body = self.parse_block_or_statement(local_names, block_locals)?;
        let else_body = if self.eat_word("else") {
            if *self.peek() == Token::KeywordIf {
                vec![self.parse_if_statement(local_names, block_locals)?]
            } else {
                self.parse_block_or_statement(local_names, block_locals)?
            }
        } else {
            Vec::new()
        };
        Ok(Statement::If { condition, then_body, else_body })
    }

    /// A `while`, `do … while`, or `for` loop. The body is a `{ … }` block or a
    /// single statement; the for-clause `init`/`step` are expressions (an `i = 0`
    /// assignment, an `i++` increment), any of which may be empty.
    fn parse_loop_statement(&mut self, local_names: &mut std::collections::HashSet<String>, block_locals: &mut Vec<LocalDeclaration>) -> Compilation<Statement> {
        match self.peek() {
            Token::KeywordWhile => {
                self.advance();
                self.expect(Token::ParenOpen)?;
                let condition = Some(self.expression()?);
                self.expect(Token::ParenClose)?;
                let body = self.parse_block_or_statement(local_names, block_locals)?;
                Ok(Statement::Loop { kind: LoopKind::While, initializer: None, condition, step: None, body })
            }
            Token::KeywordDo => {
                self.advance();
                let body = self.parse_block_or_statement(local_names, block_locals)?;
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
                let initializer = (*self.peek() != Token::Semicolon)
                    .then(|| self.comma_expression())
                    .transpose()?
                    .map(lower_discarded_post_step);
                self.expect(Token::Semicolon)?;
                let condition = (*self.peek() != Token::Semicolon).then(|| self.expression()).transpose()?;
                self.expect(Token::Semicolon)?;
                let step = (*self.peek() != Token::ParenClose)
                    .then(|| self.comma_expression())
                    .transpose()?
                    .map(lower_discarded_post_step);
                self.expect(Token::ParenClose)?;
                let body = self.parse_block_or_statement(local_names, block_locals)?;
                Ok(Statement::Loop { kind: LoopKind::For, initializer, condition, step, body })
            }
            other => Err(Diagnostic::error(format!("expected a loop keyword, found {other}"))),
        }
    }

    /// A for-clause expression list: `a = 1, b = 2` folds left into the
    /// comma operator (`for (ix = -1043, i = lx; ...)` — e_fmod, mem).
    /// Elements route through `assignment_expression` so compound forms
    /// (`i <<= 1`) parse in expression position.
    fn comma_expression(&mut self) -> Compilation<Expression> {
        let mut expression = self.assignment_expression()?;
        while self.eat_keyword(Token::Comma) {
            let right = self.assignment_expression()?;
            expression = Expression::Comma { left: Box::new(expression), right: Box::new(right) };
        }
        Ok(expression)
    }

    /// A `{ ... }` block, or a single (non-`return`) statement, as a conditional
    /// branch body.
    fn parse_block_or_statement(&mut self, local_names: &mut std::collections::HashSet<String>, block_locals: &mut Vec<LocalDeclaration>) -> Compilation<Vec<Statement>> {
        if *self.peek() == Token::BraceOpen {
            return self.parse_block(local_names, block_locals);
        }
        // An empty body — `while (c) ;` / `if (c) ;` — is no statements.
        if *self.peek() == Token::Semicolon {
            self.advance();
            return Ok(Vec::new());
        }
        if *self.peek() == Token::KeywordIf {
            return Ok(vec![self.parse_if_statement(local_names, block_locals)?]);
        }
        if *self.peek() == Token::KeywordReturn {
            return Ok(vec![self.parse_return_statement()?]);
        }
        if matches!(self.peek(), Token::KeywordWhile | Token::KeywordDo | Token::KeywordFor) {
            return Ok(vec![self.parse_loop_statement(local_names, block_locals)?]);
        }
        if let Some(statement) = self.parse_jump_statement()? {
            return Ok(vec![statement]);
        }
        Ok(vec![self.parse_simple_statement(local_names, block_locals)?])
    }

    /// A `{ ... }` block of simple statements, nested if-blocks, and `return`s. A
    /// trailing `if (c) { return X; } return Y;` collapses to `return (c ? X : Y)`
    /// (mwcc lowers an if-return followed by a return to a select), which also
    /// makes nested if-return chains fold into nested ternaries.
    fn parse_block(&mut self, local_names: &mut std::collections::HashSet<String>, block_locals: &mut Vec<LocalDeclaration>) -> Compilation<Vec<Statement>> {
        self.expect(Token::BraceOpen)?;
        let rename_depth = self.block_renames.len();
        let mut statements = Vec::new();
        while *self.peek() != Token::BraceClose {
            // An empty statement (a lone `;`) produces no code — skip it.
            if *self.peek() == Token::Semicolon {
                self.advance();
                continue;
            }
            if *self.peek() == Token::KeywordIf {
                statements.push(self.parse_if_statement(local_names, block_locals)?);
                continue;
            }
            if *self.peek() == Token::KeywordReturn {
                statements.push(self.parse_return_statement()?);
                continue;
            }
            if matches!(self.peek(), Token::KeywordWhile | Token::KeywordDo | Token::KeywordFor) {
                statements.push(self.parse_loop_statement(local_names, block_locals)?);
                continue;
            }
            if let Some(statement) = self.parse_jump_statement()? {
                statements.push(statement);
                continue;
            }
            // A nested bare `{ ... }` scoping block flattens recursively (its
            // declarations hoist through the shared block_locals).
            if *self.peek() == Token::BraceOpen {
                let mut inner = self.parse_block(local_names, block_locals)?;
                statements.append(&mut inner);
                continue;
            }
            // A BLOCK-SCOPED declaration (`f32 guess = ...;` inside an if):
            // hoist the local to the function and keep the initialization as
            // an Assign at its position (it may be conditionally reached).
            // `static` block locals defer (a named-datum shape).
            if self.peek_is_type() {
                if matches!(self.peek(), Token::Identifier(word) if word == "static") {
                    return Err(Diagnostic::error("a static local in a nested block is not supported yet (roadmap)"));
                }
                let declared_type = self.parse_type()?;
                if self.last_type_was_volatile {
                    return Err(Diagnostic::error("a volatile local is not supported yet (roadmap)"));
                }
                // A struct/union-typed local carries its tag so `cur->next` resolves
                // the layout — same as the function-top-level path. Nested-block
                // declarations (a `DestructorChain* cur` inside a while) went
                // unregistered before, so member access on them failed to type.
                let struct_tag = self.last_struct_tag.take();
                loop {
                    let mut declared_type = declared_type;
                    if self.eat_keyword(Token::Star) {
                        if *self.peek() == Token::Star {
                            return Err(Diagnostic::error("a pointer-to-pointer declarator in a nested block is not supported yet (roadmap)"));
                        }
                        declared_type = Type::Pointer(pointee_of(declared_type)?);
                    }
                    let name = self.parse_identifier()?;
                    // A shadowing declaration hoists under a fresh internal name
                    // (`i@2`); references inside the block resolve to it via the
                    // rename stack (mwcc gives the shadow its own value/slot).
                    let name = if local_names.contains(&name) {
                        self.rename_counter += 1;
                        let internal = format!("{name}@{}", self.rename_counter);
                        self.block_renames.push((name, internal.clone()));
                        internal
                    } else {
                        name
                    };
                    if *self.peek() == Token::BracketOpen {
                        return Err(Diagnostic::error("a block-scoped array is not supported yet (roadmap)"));
                    }
                    block_locals.push(LocalDeclaration { declared_type, name: name.clone(), initializer: None, array_length: None, is_static: false, data_bytes: None, is_const: false });
                    local_names.insert(name.clone());
                    // Register the type so `sizeof(s_h)` (fdlibm's __HI/__LO
                    // macros inside e_pow's inner block) resolves at parse time.
                    self.variable_types.insert(name.clone(), declared_type);
                    if let Some(tag) = &struct_tag {
                        self.variable_structs.insert(name.clone(), tag.clone());
                    }
                    if self.eat_keyword(Token::Equals) {
                        let value = self.expression()?;
                        statements.push(Statement::Assign { name, value });
                    }
                    if !self.eat_keyword(Token::Comma) {
                        break;
                    }
                }
                self.expect(Token::Semicolon)?;
                continue;
            }
            statements.push(self.parse_simple_statement(local_names, block_locals)?);
        }
        collapse_if_return_chain(&mut statements);
        self.expect(Token::BraceClose)?;
        self.block_renames.truncate(rename_depth);
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

/// Lower a value-DISCARDED postfix step (`x++` as a statement or a
/// for-clause element) to its `x = x ± 1` desugar — exact when the value
/// is unused. Comma lists lower each element.
fn lower_discarded_post_step(expression: Expression) -> Expression {
    match expression {
        Expression::PostStep { target, operator } => Expression::Assign {
            target: target.clone(),
            value: Box::new(Expression::Binary {
                operator,
                left: target,
                right: Box::new(Expression::IntegerLiteral(1)),
            }),
        },
        Expression::Comma { left, right } => Expression::Comma {
            left: Box::new(lower_discarded_post_step(*left)),
            right: Box::new(lower_discarded_post_step(*right)),
        },
        other => other,
    }
}

