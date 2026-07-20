//! Top-level item parsing: declarations, inline handling, and the translation unit.
//! Statement parsing lives in `statements`; global initializer and static-data
//! parsing in `initializers`; type and struct/union-body parsing in `types`.
//!
//! Split from the former single items.rs (fire 536); behavior-identical.

mod asm;
mod initializers;
mod statements;
mod templates;
mod types;

use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{
    ConditionalOrigin, Expression, Function, GlobalDeclaration, GuardedReturn, LocalDeclaration,
    LoopKind, Parameter, Pointee, PointerElement, Statement, SwitchArm, TranslationUnit, Type,
};
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

fn store_or_assign(
    target: Expression,
    value: Expression,
    local_names: &std::collections::HashSet<String>,
) -> Statement {
    match &target {
        Expression::Variable(name) if local_names.contains(name.as_str()) => Statement::Assign {
            name: name.clone(),
            value,
        },
        _ => Statement::Store { target, value },
    }
}

/// Retain the frontend provenance of a variable-indexed update while leaving
/// constant-index hardware-register and array accesses in their established
/// lowering paths.
fn indexed_update_value(target: &Expression, value: Expression) -> Expression {
    let variable_index = matches!(target,
        Expression::Index { index, .. }
            if crate::expressions::fold_constant_expression(index).is_err()
    );
    if variable_index {
        Expression::IndexedUpdateValue {
            value: Box::new(value),
        }
    } else {
        value
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
        other => Err(Diagnostic::error(format!(
            "pointer to {other:?} is not supported yet"
        ))),
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

pub(crate) fn type_size(declared: Type) -> u32 {
    match declared {
        Type::Pointer(_) | Type::StructPointer { .. } => 4,
        Type::Struct { size, .. } => size,
        other => (other.width() / 8) as u32,
    }
}

/// A type's alignment for laying out a struct member: a struct value aligns to its
/// own alignment (not its size), every other type to its size.
pub(crate) fn type_alignment(declared: Type) -> u32 {
    match declared {
        Type::Struct { align, .. } => align as u32,
        other => type_size(other),
    }
}

/// Whether an expression tree contains a call to any of `names`
/// (the inline-materialization and skipped-inline checks share this walk).
pub(crate) fn expression_calls(
    expression: &Expression,
    names: &std::collections::HashSet<String>,
) -> bool {
    match expression {
        Expression::Call { name, arguments } => {
            names.contains(name)
                || arguments
                    .iter()
                    .any(|argument| expression_calls(argument, names))
        }
        Expression::Binary { left, right, .. } => {
            expression_calls(left, names) || expression_calls(right, names)
        }
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::BitFieldRead {
            extracted: operand, ..
        }
        | Expression::AddressOf { operand } => expression_calls(operand, names),
        Expression::Dereference { pointer } => expression_calls(pointer, names),
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
            expression_calls(base, names)
        }
        Expression::Index { base, index } => {
            expression_calls(base, names) || expression_calls(index, names)
        }
        Expression::Assign { target, value } => {
            expression_calls(target, names) || expression_calls(value, names)
        }
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            expression_calls(condition, names)
                || expression_calls(when_true, names)
                || expression_calls(when_false, names)
        }
        _ => false,
    }
}
pub(crate) fn statement_calls(
    statement: &Statement,
    names: &std::collections::HashSet<String>,
) -> bool {
    match statement {
        Statement::Store { target, value } => {
            expression_calls(target, names) || expression_calls(value, names)
        }
        Statement::Assign { value, .. } => expression_calls(value, names),
        Statement::Expression(expression) => expression_calls(expression, names),
        Statement::If {
            condition,
            then_body,
            else_body,
        } => {
            expression_calls(condition, names)
                || then_body.iter().any(|inner| statement_calls(inner, names))
                || else_body.iter().any(|inner| statement_calls(inner, names))
        }
        Statement::Switch {
            scrutinee,
            arms,
            default,
        } => {
            expression_calls(scrutinee, names)
                || arms.iter().any(|arm| match &arm.body {
                    mwcc_syntax_trees::ArmBody::Return(result) => expression_calls(result, names),
                    mwcc_syntax_trees::ArmBody::Statements(statements) => statements
                        .iter()
                        .any(|statement| statement_calls(statement, names)),
                })
                || default.as_ref().is_some_and(|body| match body {
                    mwcc_syntax_trees::ArmBody::Return(expression) => {
                        expression_calls(expression, names)
                    }
                    mwcc_syntax_trees::ArmBody::Statements(statements) => statements
                        .iter()
                        .any(|statement| statement_calls(statement, names)),
                })
        }
        Statement::Return(Some(expression)) => expression_calls(expression, names),
        Statement::Loop {
            initializer,
            condition,
            step,
            body,
            ..
        } => {
            initializer
                .as_ref()
                .is_some_and(|expression| expression_calls(expression, names))
                || condition
                    .as_ref()
                    .is_some_and(|expression| expression_calls(expression, names))
                || step
                    .as_ref()
                    .is_some_and(|expression| expression_calls(expression, names))
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
    /// Consume surfaced file-scope pragmas and update parser state. Keeping this
    /// at translation-unit level lets a language pragma affect the namespace or
    /// declaration that follows it instead of being mistaken for part of that item.
    fn consume_top_level_pragmas(&mut self) {
        while let Token::Pragma(directive) = self.peek() {
            match directive.as_str() {
                "push" => self.cplusplus_stack.push(self.cplusplus),
                "pop" => {
                    self.cplusplus = self.cplusplus_stack.pop().unwrap_or(self.default_cplusplus)
                }
                "cplusplus on" => self.cplusplus = true,
                "cplusplus off" => self.cplusplus = false,
                "cplusplus reset" => self.cplusplus = self.default_cplusplus,
                "defer_codegen on" => self.defer_codegen = true,
                "defer_codegen off" => self.defer_codegen = false,
                "force_active on" => self.force_active = true,
                "force_active off" | "force_active reset" => self.force_active = false,
                "peephole off" => self.peephole_disabled = true,
                "peephole on" | "peephole reset" => self.peephole_disabled = false,
                _ => {}
            }
            self.advance();
        }
    }

    /// Consume an identifier token if it matches `word` (used for the `long` and
    /// `signed`/`unsigned` specifier words that aren't dedicated keywords).
    pub(crate) fn eat_word(&mut self, word: &str) -> bool {
        if matches!(self.peek(), Token::Identifier(found) if found == word) {
            self.advance();
            true
        } else {
            false
        }
    }

    /// Consume `token` if it is next; report whether it was.
    pub(crate) fn eat_keyword(&mut self, token: Token) -> bool {
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
    pub(crate) fn parse_enum_body(&mut self) -> Compilation<(i64, i64)> {
        self.expect(Token::BraceOpen)?;
        let mut next = 0i64;
        let mut minimum = i64::MAX;
        let mut maximum = i64::MIN;
        while *self.peek() != Token::BraceClose {
            let name = self.parse_identifier()?;
            let value = if self.eat_keyword(Token::Equals) {
                self.parse_enum_value()?
            } else {
                next
            };
            self.enum_constants.insert(name, value);
            minimum = minimum.min(value);
            maximum = maximum.max(value);
            next = value + 1;
            if *self.peek() == Token::Comma {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(Token::BraceClose)?;
        Ok((minimum, maximum))
    }

    /// Evaluate a constant enumerator expression — integer/char literals, prior
    /// enumerators, parentheses, and left-to-right `+ - * & | ^ << >>`.
    pub(crate) fn parse_enum_value(&mut self) -> Compilation<i64> {
        let mut value = self.parse_enum_primary()?;
        loop {
            value = match self.peek() {
                Token::Plus => {
                    self.advance();
                    value + self.parse_enum_primary()?
                }
                Token::Minus => {
                    self.advance();
                    value - self.parse_enum_primary()?
                }
                Token::Star => {
                    self.advance();
                    value * self.parse_enum_primary()?
                }
                Token::Ampersand => {
                    self.advance();
                    value & self.parse_enum_primary()?
                }
                Token::Pipe => {
                    self.advance();
                    value | self.parse_enum_primary()?
                }
                Token::Caret => {
                    self.advance();
                    value ^ self.parse_enum_primary()?
                }
                Token::ShiftLeft => {
                    self.advance();
                    value << self.parse_enum_primary()?
                }
                Token::ShiftRight => {
                    self.advance();
                    value >> self.parse_enum_primary()?
                }
                _ => break,
            };
        }
        Ok(value)
    }

    pub(crate) fn parse_enum_primary(&mut self) -> Compilation<i64> {
        let negative = self.eat_keyword(Token::Minus);
        let value = match self.advance() {
            Token::IntegerLiteral(value) => value,
            Token::Identifier(name) => *self.enum_constants.get(&name).ok_or_else(|| {
                Diagnostic::error(format!("non-constant enumerator value '{name}'"))
            })?,
            Token::ParenOpen => {
                let value = self.parse_enum_value()?;
                self.expect(Token::ParenClose)?;
                value
            }
            other => {
                return Err(Diagnostic::error(format!(
                    "expected an enumerator value, found {other}"
                )))
            }
        };
        Ok(if negative { -value } else { value })
    }

    /// A constant integer in statement position — a `switch` case label. Parsed as a
    /// full constant expression so an enum constant (`case GX_MODULATE:`) or a folded
    /// expression (`case A | B:`) resolves, not just a bare integer literal.
    pub(crate) fn parse_integer_constant(&mut self) -> Compilation<i64> {
        let expression_start = self.position;
        let expression = self.expression()?;
        crate::expressions::fold_constant_expression(&expression).map_err(|error| {
            if std::env::var_os("MWCC_PARSE_DEBUG").is_some() {
                eprintln!(
                    "non-constant integer expression at {} (tokens {expression_start}..{}): {expression:?}",
                    self.diagnostic_position(expression_start),
                    self.position,
                );
            }
            error
        })
    }

    /// Parse `switch (scrutinee) { case <int>: return E; ... default: return E; }`.
    /// The subset requires every arm to be a single `return`; fall-through, blocks,
    /// and non-constant case labels are not supported yet.
    pub(crate) fn parse_switch(
        &mut self,
        local_names: &mut std::collections::HashSet<String>,
        block_locals: &mut Vec<LocalDeclaration>,
    ) -> Compilation<Statement> {
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
                let (body, falls_through) =
                    self.parse_switch_arm_body(local_names, block_locals)?;
                arms.push(SwitchArm {
                    value,
                    body,
                    falls_through,
                });
            } else if self.eat_word("default") {
                self.expect(Token::Colon)?;
                let (body, _falls_through) =
                    self.parse_switch_arm_body(local_names, block_locals)?;
                default = Some(body);
            } else if matches!(self.peek(), Token::Identifier(_))
                && *self.peek_at(1) == Token::Colon
            {
                // A goto LABEL between arms (scanf's `signed_int:`) — control
                // reaches it by falling through the previous arm or by goto, so
                // the label and its statements continue that arm's body.
                let name = self.parse_identifier()?;
                self.advance(); // the colon
                let (continuation, falls_through) =
                    self.parse_switch_arm_body(local_names, block_locals)?;
                let Some(last) = arms.last_mut() else {
                    return Err(Diagnostic::error(
                        "a goto label before the first switch arm is not supported yet (roadmap)",
                    ));
                };
                let mut statements = match std::mem::replace(
                    &mut last.body,
                    mwcc_syntax_trees::ArmBody::Statements(Vec::new()),
                ) {
                    mwcc_syntax_trees::ArmBody::Return(expression) => {
                        vec![Statement::Return(Some(expression))]
                    }
                    mwcc_syntax_trees::ArmBody::Statements(statements) => statements,
                };
                statements.push(Statement::Label(name));
                match continuation {
                    mwcc_syntax_trees::ArmBody::Return(expression) => {
                        statements.push(Statement::Return(Some(expression)))
                    }
                    mwcc_syntax_trees::ArmBody::Statements(inner) => statements.extend(inner),
                }
                last.body = mwcc_syntax_trees::ArmBody::Statements(statements);
                last.falls_through = falls_through;
            } else {
                return Err(Diagnostic::error(format!("a switch arm must be `case <int>: return …;` or `default: return …;` (roadmap; found {})", self.peek())));
            }
        }
        self.expect(Token::BraceClose)?;
        Ok(Statement::Switch {
            scrutinee,
            arms,
            default,
        })
    }

    /// A switch arm's body: the common `return E;` (optionally braced, with
    /// dead trailing `break;`s), or a braced STATEMENT body ending at its
    /// `break;` — represented faithfully (mwcc branches these; a ternary
    /// lowering is byte-different).
    pub(crate) fn parse_switch_arm_body(
        &mut self,
        local_names: &mut std::collections::HashSet<String>,
        block_locals: &mut Vec<LocalDeclaration>,
    ) -> Compilation<(mwcc_syntax_trees::ArmBody, bool)> {
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
                // `case X: { return E; } break;` — a redundant break AFTER the
                // braces still belongs to this arm.
                if matches!(self.peek(), Token::Identifier(word) if word == "break") {
                    self.advance();
                    self.expect(Token::Semicolon)?;
                }
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
                // Dead consecutive breaks (`break; break;` — sunshine's
                // inverse_trig index ladder) all belong to this arm.
                while matches!(self.peek(), Token::Identifier(word) if word == "break") {
                    self.advance();
                    self.expect(Token::Semicolon)?;
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
                // `case X: { ... } break;` — a break AFTER the braces ends
                // this arm (strtold's per-state blocks).
                if matches!(self.peek(), Token::Identifier(word) if word == "break") {
                    self.advance();
                    self.expect(Token::Semicolon)?;
                    saw_break = true;
                }
                break;
            }
            if *self.peek() == Token::KeywordIf {
                statements.push(self.parse_if_statement(local_names, block_locals)?);
                continue;
            }
            // A NESTED switch inside an arm (strtold's per-state character
            // dispatch) recurses like any other statement.
            if matches!(self.peek(), Token::Identifier(word) if word == "switch") {
                statements.push(self.parse_switch(local_names, block_locals)?);
                continue;
            }
            if *self.peek() == Token::KeywordReturn {
                statements.push(self.parse_return_statement()?);
                continue;
            }
            if matches!(
                self.peek(),
                Token::KeywordWhile | Token::KeywordDo | Token::KeywordFor
            ) {
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
            // A declaration inside a braced arm (`case 'N': { double result;
            // u64* ll = (u64*)&result; … }` — bfbb ansi_fp's __dec2num) hoists
            // exactly like one in a nested block.
            if self.peek_is_type()
                || self.peek_is_local_array_typedef()
                || matches!(self.peek(), Token::Identifier(word) if word == "static")
            {
                self.parse_block_declaration(local_names, block_locals, &mut statements)?;
                continue;
            }
            statements.push(self.parse_simple_statement(local_names, block_locals)?);
        }
        let falls_through = !saw_break
            && !matches!(
                statements.last(),
                Some(Statement::Return(_) | Statement::Goto(_))
            );
        Ok((ArmBody::Statements(statements), falls_through))
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
            self.consume_top_level_pragmas();
            if *self.peek() == Token::EndOfFile {
                break;
            }
            // Namespace braces delimit declaration scopes rather than ordinary
            // statements. Keep the scope explicitly so class-member symbols use
            // CodeWarrior's nested `Qn` encoding, while the existing top-level item
            // parser can continue consuming the declarations inside the wrapper.
            if self.cplusplus && self.eat_word("namespace") {
                // An anonymous namespace has internal linkage but no ABI scope
                // spelling in this compiler family. Retain an empty stack entry
                // solely so its closing brace is paired as a declaration scope.
                let namespace = if *self.peek() == Token::BraceOpen {
                    String::new()
                } else {
                    self.parse_identifier()?
                };
                self.expect(Token::BraceOpen)?;
                if !namespace.is_empty() {
                    let named_parent = self
                        .namespace_stack
                        .iter()
                        .filter(|scope| !scope.is_empty())
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("::");
                    let qualified = if named_parent.is_empty() {
                        namespace.clone()
                    } else {
                        format!("{named_parent}::{namespace}")
                    };
                    self.cxx_namespaces.insert(qualified);
                }
                self.namespace_stack.push(namespace);
                continue;
            }
            if *self.peek() == Token::BraceClose && !self.namespace_stack.is_empty() {
                self.advance();
                self.namespace_stack.pop();
                self.eat_keyword(Token::Semicolon);
                continue;
            }
            // Linkage blocks normalized from `extern "C" { ... };` can leave the
            // optional trailing semicolon. It is an empty declaration, not an
            // unparseable item worth reporting through recovery.
            if self.eat_keyword(Token::Semicolon) {
                continue;
            }
            // An explicit specialization emits as a concrete declaration or
            // definition. Preserve the prefix long enough for inline-template
            // recovery to classify unused member definitions, then let the
            // ordinary item parser handle the concrete item that follows it.
            let skippable_inline_member = self.item_is_skippable_inline_member_definition();
            let explicit_specialization = !skippable_inline_member
                && self.consume_explicit_specialization_prefix();
            let explicit_data_specialization = explicit_specialization
                && self.item_is_explicit_data_specialization();
            let start = self.position;
            // Inline is declaration state, not layout state. Capture it before
            // either the C++ layout parser succeeds or recovery skips a class.
            prototypes.extend(self.capture_cxx_class_declarations());
            let functions_before = functions.len();
            let globals_before = globals.len();
            let bump_before_item = self.skipped_inline_functions;
            let item_result = if skippable_inline_member {
                // Route definitions whose inherited inline status was proven by
                // declaration recovery through the same dropped-inline accounting
                // as definitions carrying a written `inline` keyword.
                Err(Diagnostic::error(
                    "deferred unused C++ inline member materialization",
                ))
            } else {
                self.parse_top_level_item(&mut globals, &mut functions, &mut prototypes)
            };
            if let Err(error) = item_result {
                if std::env::var_os("MWCC_CAPTURE_DEBUG").is_some() {
                    eprintln!(
                        "skipped top-level item at token {start} ({:?}): {error}",
                        self.tokens.get(start)
                    );
                }
                // A declaration we can't parse (a typedef/struct/extern prototype or
                // qualified type from a preprocessed header) is skipped so the
                // function definitions can still be compiled; a function definition we
                // are expected to compile is propagated, deferring the unit honestly.
                self.position = start;
                // An unparsed explicit specialization is concrete, not a primary
                // template declaration. Static data-member specializations such
                // as `template <> Pool<T> Owner<T>::pool;` emit storage, startup
                // code, constructors, and weak template bodies. Skipping one
                // produces a plausible prefix object with the entire generated
                // tail missing, so keep this an honest DEFER until instantiation
                // lowering owns the full emission graph.
                if explicit_data_specialization {
                    return Err(Diagnostic::error(format!(
                        "an explicit C++ template specialization was not lowered: {error}"
                    )));
                }
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
                if let Some((name, is_static)) = self.inline_asm_function_name() {
                    if is_static {
                        self.inline_asm_symbols.push(name);
                    } else {
                        // A PLAIN inline asm helper (OSFastCast's `inline __OSf32tos16`)
                        // becomes a GLOBAL UND that mwcc materializes from the dropped
                        // compilation. The general codegen path does not emit it, so a
                        // non-captured object that carries one must DEFER (byte-exact-or-defer).
                        self.plain_inline_asm_helpers.push(name);
                    }
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
                    let (function_name, is_static_inline, statics) =
                        self.parse_skipped_inline_statics()?;
                    if is_static_inline {
                        // Positional numbering: sample the running bump BEFORE this
                        // inline's own counts apply — the static declares inside it.
                        for local in &statics {
                            self.static_local_prebumps
                                .insert(local.name.clone(), self.skipped_inline_functions);
                        }
                        self.skipped_inline_functions += statics.len();
                    } else {
                        for (slot, local) in statics.into_iter().enumerate() {
                            let mangled = format!(
                                "{}$localstatic{}${}",
                                local.name,
                                slot + usize::from(self.plain_inline_localstatic_base),
                                function_name
                            );
                            self.global_sizes
                                .insert(mangled.clone(), (local.byte_size as u32, None));
                            globals.push(GlobalDeclaration {
                                non_static_functions_before: functions
                                    .iter()
                                    .filter(|function| !function.is_static)
                                    .count(),
                                functions_before: functions.len(),
                                declared_type: local.declared_type,
                                name: mangled,
                                is_extern: false,
                                is_static: false,
                                array_length: None,
                                array_length_inferred: false,
                                initializer: None,
                                is_const: local.is_const,
                                address_initializer: None,
                                data_bytes: local.bytes,
                                data_relocations: Vec::new(),
                                is_weak: true,
                                section: None,
                                attribute_alignment: None,
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
                self.capture_skipped_struct_template();
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
                        self.static_local_prebumps
                            .insert(local.name.clone(), bump_before_item);
                    }
                }
            }
            // A `static` global declared AFTER a function needs its local symbol
            // interleaved at its source position among the functions' `@N` entries. The
            // writer already models this (each global carries `functions_before`, and
            // writer.rs's per-function static run at :865 emits it at that slot) — it was
            // proven for `const` statics (ansi_fp's `unused`) and holds for non-const
            // statics too: init/uninit, single/multiple, arrays, referenced or not, with
            // or without pools, and tail declarations (main.rs clamps those) all go
            // byte-exact. So no defer is needed here.
            let _ = seen_function;
        }
        if !self.namespace_stack.is_empty() {
            return Err(Diagnostic::error("unterminated C++ namespace"));
        }
        // Non-constant float-array globals synthesize a startup initializer:
        // `__sinit_ctx_c` (named for the TU) assigns each unfolded element,
        // appended LAST among the functions, plus an ANONYMOUS `.ctors` entry
        // holding an ADDR32 to it (measured: sunshine trigf — the sinit is a
        // LOCAL symbol at the end of .text; the .ctors word has no own symbol).
        if !self.pending_sinit.is_empty() {
            let statements: Vec<Statement> = self
                .pending_sinit
                .drain(..)
                .map(|(array, index, expression)| Statement::Store {
                    target: Expression::Index {
                        base: Box::new(Expression::Variable(array)),
                        index: Box::new(Expression::IntegerLiteral(index as i64)),
                    },
                    value: expression,
                })
                .collect();
            functions.push(Function {
                return_type: Type::Void,
                name: "__sinit_ctx_c".to_string(),
                is_static: true,
                is_weak: false,
                text_deferred: false,
                peephole_disabled: false,
                parameters: Vec::new(),
                locals: Vec::new(),
                statements,
                guards: Vec::new(),
                return_expression: None,
                section: None,
                asm_body: None,
                force_active: false,
            });
            self.function_sources.push(None);
            globals.push(GlobalDeclaration {
                declared_type: Type::Int,
                name: String::new(),
                is_extern: false,
                is_static: false,
                is_weak: false,
                non_static_functions_before: functions.iter().filter(|f| !f.is_static).count(),
                functions_before: functions.len(),
                array_length: None,
                array_length_inferred: false,
                initializer: None,
                is_const: true,
                address_initializer: Some(vec![PointerElement::Symbol(
                    "__sinit_ctx_c".to_string(),
                )]),
                data_bytes: None,
                data_relocations: Vec::new(),
                section: Some(".ctors".to_string()),
                attribute_alignment: None,
            });
        }
        debug_assert_eq!(
            functions.len(),
            self.function_sources.len(),
            "every parsed function must retain one source-provenance slot"
        );
        Ok(TranslationUnit {
            globals,
            functions,
            prototypes,
            named_prototype_parameters: self.named_prototype_parameters,
            inline_asm_symbols: std::mem::take(&mut self.inline_asm_symbols),
            plain_inline_asm_helpers: std::mem::take(&mut self.plain_inline_asm_helpers),
            skipped_inline_functions: self.skipped_inline_functions,
            cxx_inline_ordinal_facts: self.cxx_inline_ordinal_facts,
            static_local_prebumps: std::mem::take(&mut self.static_local_prebumps),
            implicitly_materialized: std::mem::take(&mut self.implicitly_materialized),
            materialized_inline_candidates: std::mem::take(
                &mut self.materialized_inline_candidates,
            ),
            weak_materialized: std::mem::take(&mut self.weak_materialized),
            section_prototypes: std::mem::take(&mut self.section_prototype_order),
            skipped_inline_names: std::mem::take(&mut self.skipped_inline_names),
            deferred_function_names: std::mem::take(&mut self.deferred_function_names),
            variadic_definitions: std::mem::take(&mut self.variadic_definitions),
            fixed_address_arrays: std::mem::take(&mut self.fixed_address_arrays),
            fixed_address_objects: std::mem::take(&mut self.fixed_address_globals)
                .into_iter()
                .map(|(name, (address, _cast_target, _tag))| (name, address))
                .collect(),
            function_sources: std::mem::take(&mut self.function_sources),
        })
    }

    /// If the item at the cursor is an `inline`/`static inline` function whose body
    /// contains an inline `asm` block, return its name (mwcc emits a local symbol
    /// for it). Pure lookahead — consumes nothing.
    /// Try to parse the inline definition at the cursor as
    /// `inline T name(T a, ...) { return expr; }` and record its body for
    /// call-site substitution. Restores the cursor either way.
    pub(crate) fn try_record_inline_body(&mut self) {
        let saved = self.position;
        let recorded = (|| -> Option<(String, Vec<String>, Expression)> {
            while matches!(self.peek(), Token::Identifier(word) if word == "static" || word == "inline" || word == "__inline")
            {
                self.advance();
            }
            self.parse_type().ok()?;
            // An array-typedef parameter's subscripts need the row stride, which
            // inline substitution does not carry — don't record such a body.
            if self.last_array_typedef.take().is_some() {
                return None;
            }
            let name = match self.advance().clone() {
                Token::Identifier(name) => name,
                _ => return None,
            };
            if *self.peek() != Token::ParenOpen {
                return None;
            }
            self.advance();
            let mut parameters = Vec::new();
            if *self.peek() == Token::KeywordVoid
                && self.tokens.get(self.position + 1) == Some(&Token::ParenClose)
            {
                self.advance();
            } else if *self.peek() != Token::ParenClose {
                loop {
                    self.parse_type().ok()?;
                    if self.last_array_typedef.take().is_some() {
                        return None;
                    }
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
    pub(crate) fn skipped_function_name(&self) -> Option<String> {
        let mut index = self.position;
        let mut name: Option<String> = None;
        while let Some(token) = self.tokens.get(index) {
            match token {
                Token::Identifier(word)
                    if word != "inline"
                        && word != "__inline"
                        && word != "static"
                        && word != "extern" =>
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
    pub(crate) fn aggregate_literal(&mut self) -> Compilation<Expression> {
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

    pub(crate) fn inline_asm_function_name(&self) -> Option<(String, bool)> {
        let mut index = self.position;
        let mut is_inline = false;
        let mut is_static = false;
        let mut name: Option<String> = None;
        // Signature up to the first `(`: note `static`/`inline`, and the last
        // identifier before the `(` (the function name).
        while let Some(token) = self.tokens.get(index) {
            match token {
                Token::Identifier(word) if word == "inline" || word == "__inline" => {
                    is_inline = true
                }
                Token::Identifier(word) if word == "static" => is_static = true,
                Token::Identifier(word) => name = Some(word.clone()),
                Token::ParenOpen => break,
                Token::Semicolon | Token::BraceOpen | Token::EndOfFile => return None,
                _ => {}
            }
            index += 1;
        }
        // A STATIC inline asm helper becomes the early local-UND symbol (the
        // measured OSFastCast.h shape); a PLAIN inline one (strikers' __frsqrte,
        // OSFastCast's `inline __OSf32tos16`) is a GLOBAL external mwcc creates
        // from the dropped compilation. Both are returned (with is_static) so the
        // caller records the static ones as local-UND and the plain ones for the
        // general-codegen defer check (captures declare plain ones via phantom_externals).
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
                Token::Asm => has_asm = true,
                Token::Identifier(word) if word == "__asm" => has_asm = true,
                Token::EndOfFile => break,
                _ => {}
            }
            index += 1;
        }
        has_asm.then_some((name, is_static))
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
    pub(crate) fn parse_skipped_inline_statics(
        &self,
    ) -> Compilation<(String, bool, Vec<SkippedStaticLocal>)> {
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
                Token::Identifier(word) if self.typedefs.get(word) == Some(&Type::Float) => {
                    param_codes.push('f')
                }
                Token::Identifier(word) if word == "double" => param_codes.push('d'),
                Token::Identifier(word) if self.typedefs.get(word) == Some(&Type::Double) => {
                    param_codes.push('d')
                }
                Token::KeywordInt => param_codes.push('i'),
                Token::Identifier(word) if self.typedefs.get(word) == Some(&Type::Int) => {
                    param_codes.push('i')
                }
                Token::KeywordVoid => param_codes.push('v'),
                Token::Star => {
                    return Err(Diagnostic::error(
                        "a pointer parameter in a mangled inline is not supported yet (roadmap)",
                    ));
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
                    while matches!(self.tokens.get(index), Some(Token::Identifier(word)) if word == "const" || word == "volatile")
                    {
                        if matches!(self.tokens.get(index), Some(Token::Identifier(word)) if word == "const")
                        {
                            is_const = true;
                        }
                        index += 1;
                    }
                    // The type: one keyword/typedef token (compound int forms defer),
                    // plus the `unsigned char`/`unsigned int` pairs.
                    if matches!(self.tokens.get(index), Some(Token::KeywordUnsigned))
                        && matches!(
                            self.tokens.get(index + 1),
                            Some(Token::KeywordChar | Token::KeywordInt)
                        )
                    {
                        index += 1;
                    }
                    let declared_type = match self.tokens.get(index) {
                        Some(Token::Identifier(word)) if word == "double" => Type::Double,
                        Some(Token::KeywordFloat) => Type::Float,
                        Some(Token::KeywordInt) => Type::Int,
                        Some(Token::Identifier(word)) if self.typedefs.get(word) == Some(&Type::Double) => Type::Double,
                        Some(Token::Identifier(word)) if self.typedefs.get(word) == Some(&Type::Float) => Type::Float,
                        Some(Token::Identifier(word)) if self.typedefs.get(word) == Some(&Type::Int) => Type::Int,
                        Some(Token::Identifier(word)) if self.typedefs.get(word) == Some(&Type::UnsignedInt) => Type::UnsignedInt,
                        // A char static (strikers _alloc's `static char init;`).
                        Some(Token::KeywordChar) => Type::Char,
                        // A struct-typed static (`static __mem_pool protopool;`)
                        // carries its own layout size (typedef name -> tag ->
                        // declared layout).
                        Some(Token::Identifier(word)) if matches!(self.typedefs.get(word), Some(Type::Struct { .. })) => *self.typedefs.get(word).unwrap(),
                        Some(Token::Identifier(word))
                            if self
                                .struct_typedefs
                                .get(word)
                                .and_then(|tag| self.structs.get(tag))
                                .is_some() =>
                        {
                            let layout = &self.structs[&self.struct_typedefs[word]];
                            Type::Struct { size: layout.size, align: layout.align }
                        }
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
                                (Some(Token::IntegerLiteral(value)), Type::Char | Type::UnsignedChar) => {
                                    let value = if negative { -*value } else { *value };
                                    if value == 0 { None } else { Some(vec![value as u8]) }
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
                        Type::Char | Type::UnsignedChar => 1,
                        Type::Struct { size, .. } => u16::try_from(size).map_err(|_| {
                            Diagnostic::error("a skipped static local exceeds 65535 bytes")
                        })?,
                        _ => 4,
                    };
                    statics.push(SkippedStaticLocal {
                        name: local_name,
                        declared_type,
                        is_const,
                        bytes,
                        byte_size,
                    });
                    continue;
                }
                Token::EndOfFile => break,
                _ => {}
            }
            index += 1;
        }
        Ok((name, is_static_inline, statics))
    }

    pub(crate) fn inline_function_has_static_local(&self) -> bool {
        let mut index = self.position;
        let mut is_inline = false;
        // Signature up to the first `(`: note `inline` (an `extern`/`static` qualifier
        // may precede it). A `;`/`{`/EOF before the `(` means this is not a function.
        while let Some(token) = self.tokens.get(index) {
            match token {
                Token::Identifier(word) if word == "inline" || word == "__inline" => {
                    is_inline = true
                }
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
    pub(crate) fn parse_top_level_item(
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
            self.consume_top_level_pragmas();
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
                                Token::Identifier(inner) if inner == "section" => {
                                    saw_section_kw = true
                                }
                                Token::StringLiteral(bytes) if saw_section_kw => {
                                    declspec_section =
                                        Some(String::from_utf8_lossy(&bytes).into_owned());
                                    saw_section_kw = false;
                                }
                                Token::EndOfFile => {
                                    return Err(Diagnostic::error("unterminated __declspec"))
                                }
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
            if self.cplusplus
                && matches!(self.peek(), Token::Identifier(word) if word == "class")
                && matches!(self.peek_at(1), Token::Identifier(_))
            {
                let (name, layout, class) = self.parse_class_definition()?;
                let class_type = Type::StructPointer {
                    element_size: layout.size,
                };
                for signature in &class.constructors {
                    let mangled = self.mangle_typed_member_in_current_namespace(
                        &name,
                        "__ct",
                        &signature.cxx_parameters,
                    )?;
                    let mut parameter_types = vec![class_type];
                    parameter_types.extend(signature.parameters.iter().copied());
                    prototypes.push((mangled, class_type, parameter_types));
                }
                self.struct_typedefs.insert(name.clone(), name.clone());
                self.structs.insert(name.clone(), layout);
                self.cxx_classes.insert(name, class);
                return Ok(());
            }
            // A Metrowerks inline-`asm` function DEFINITION: `[static] asm <ret>
            // name(params) { <instructions> }` or `[static] <ret> asm name(params)`.
            // Its body is assembled verbatim (no C
            // codegen), so it is parsed by its own path. A bodyless `asm` prototype
            // yields no definition. An `inline` asm function is NOT handled here — it
            // is a skipped inline helper (recorded as a local-UND symbol by the
            // error-recovery path), never emitted. (The `static`/`__declspec(weak)`
            // qualifiers already ran.)
            if *self.peek() == Token::Asm && !is_inline {
                if let Some(function) = self.parse_asm_function(is_static, is_weak, false)? {
                    functions.push(function);
                }
                return Ok(());
            }
            let asm_follows_return_type = !is_inline
                && self.tokens[self.position..]
                    .iter()
                    .take_while(|token| {
                        !matches!(
                            token,
                            Token::ParenOpen
                                | Token::Semicolon
                                | Token::BraceOpen
                                | Token::EndOfFile
                        )
                    })
                    .any(|token| *token == Token::Asm);
            if asm_follows_return_type {
                if let Some(function) = self.parse_asm_function(is_static, is_weak, true)? {
                    functions.push(function);
                }
                return Ok(());
            }
            // `typedef <type> <name>;` registers a type alias. (Function-pointer and
            // array typedefs are not in the subset yet.)
            // Preserve the primary-template identity even when layout recovery
            // makes the ordinary typedef parser succeed. Error recovery already
            // does this for opaque template instances; the successful path needs
            // the same fact so a later explicit member specialization inherits
            // its class-body (implicit-inline) status.
            if matches!(self.peek(), Token::Identifier(word) if word == "typedef") {
                self.capture_template_alias();
            }
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
                    let tag = if matches!(self.peek(), Token::Identifier(_)) {
                        self.parse_identifier()?
                    } else {
                        String::new()
                    };
                    let mut layout = if is_union_kw {
                        self.parse_union_body()?
                    } else {
                        self.parse_struct_body()?
                    };
                    if let Some(align) = self.skip_attributes()? {
                        layout.align = layout.align.max(align as u8);
                        let align = u32::from(align);
                        layout.size = layout.size.div_ceil(align) * align;
                    }
                    // One or more comma-separated declarators: a value alias `Vec`
                    // or a pointer alias `*VecPtr`. The first value alias names an
                    // anonymous struct's tag.
                    let mut is_pointer = self.eat_keyword(Token::Star);
                    let mut alias = self.parse_identifier()?;
                    let tag = if tag.is_empty() { alias.clone() } else { tag };
                    loop {
                        // An ARRAY declarator (`typedef struct {…} __va_list[1];` — the
                        // stdarg va_list shape): the alias still resolves through the
                        // struct tag; a parameter of this type decays to the struct
                        // pointer exactly like the bare struct typedef does.
                        while *self.peek() == Token::BracketOpen {
                            self.advance();
                            self.parse_integer_constant()?;
                            self.expect(Token::BracketClose)?;
                        }
                        // MWCC accepts a GNU attribute after the typedef alias,
                        // but unlike one before the alias it does not alter the
                        // aliased aggregate's `sizeof`/natural alignment.
                        self.skip_attributes()?;
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
                    self.structs.insert(tag.clone(), layout);
                    self.expect(Token::Semicolon)?;
                    return Ok(());
                }
                // A BODYLESS `typedef struct Tag Alias;` (a forward typedef —
                // the layout arrives when `struct Tag { ... }` is defined) or
                // `typedef struct Tag* AliasPtr;` registers the alias->TAG map
                // directly; member lookups resolve through the tag at use time.
                let is_union_forward =
                    matches!(self.peek(), Token::Identifier(word) if word == "union");
                if (*self.peek() == Token::KeywordStruct || is_union_forward)
                    && matches!(
                        self.tokens.get(self.position + 1),
                        Some(Token::Identifier(_))
                    )
                    && matches!(
                        (
                            self.tokens.get(self.position + 2),
                            self.tokens.get(self.position + 3)
                        ),
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
                // `typedef Existing NewAlias;` — a pure re-alias of a struct,
                // struct-pointer, or array typedef COPIES the original registration
                // (`typedef __va_list va_list;` — parse_type's scalar model would
                // lose the struct identity).
                if let (
                    Token::Identifier(existing),
                    Some(Token::Identifier(_)),
                    Some(Token::Semicolon | Token::BracketOpen),
                ) = (
                    self.peek(),
                    self.tokens.get(self.position + 1),
                    self.tokens.get(self.position + 2),
                ) {
                    let existing = existing.clone();
                    let struct_tag = self.struct_typedefs.get(&existing).cloned();
                    let pointer_tag = self.struct_pointer_typedefs.get(&existing).cloned();
                    let array_entry = self.array_typedefs.get(&existing).cloned();
                    let function_pointer = self.function_pointer_typedefs.contains(&existing);
                    if struct_tag.is_some()
                        || pointer_tag.is_some()
                        || array_entry.is_some()
                        || function_pointer
                    {
                        self.advance(); // the existing alias
                        let alias = self.parse_identifier()?;
                        // An ARRAY declarator on the re-alias (`typedef _va_list_struct
                        // __va_list[1];` — wind_waker's stdarg spelling): the alias still
                        // resolves through the struct tag; a parameter decays to the
                        // struct pointer exactly like the bare struct typedef.
                        while *self.peek() == Token::BracketOpen {
                            self.advance();
                            self.parse_integer_constant()?;
                            self.expect(Token::BracketClose)?;
                        }
                        self.expect(Token::Semicolon)?;
                        if let Some(tag) = struct_tag {
                            self.struct_typedefs.insert(alias.clone(), tag);
                        }
                        if let Some(tag) = pointer_tag {
                            self.struct_pointer_typedefs.insert(alias.clone(), tag);
                        }
                        if let Some(entry) = array_entry {
                            self.array_typedefs.insert(alias.clone(), entry);
                        }
                        if function_pointer {
                            self.function_pointer_typedefs.insert(alias);
                        }
                        return Ok(());
                    }
                }
                let aliased = self.parse_type()?;
                // `typedef RET (*name)(params);` (function pointer, a 4-byte word
                // pointer) or `typedef T (*name)[N];` (pointer to array — a ROW
                // pointer whose subscript strides by N elements).
                if *self.peek() == Token::ParenOpen
                    && self.tokens.get(self.position + 1) == Some(&Token::Star)
                {
                    self.advance(); // `(`
                    self.advance(); // `*`
                    let alias = self.parse_identifier()?;
                    self.expect(Token::ParenClose)?;
                    if *self.peek() == Token::BracketOpen {
                        self.advance(); // `[`
                        let length = self.parse_integer_constant()? as u16;
                        self.expect(Token::BracketClose)?;
                        self.expect(Token::Semicolon)?;
                        self.row_pointer_typedefs.insert(alias, (aliased, length));
                        return Ok(());
                    }
                    self.expect(Token::ParenOpen)?;
                    let mut depth = 1;
                    while depth > 0 {
                        match self.advance() {
                            Token::ParenOpen => depth += 1,
                            Token::ParenClose => depth -= 1,
                            Token::EndOfFile => {
                                return Err(Diagnostic::error(
                                    "unterminated function-pointer typedef",
                                ))
                            }
                            _ => {}
                        }
                    }
                    self.expect(Token::Semicolon)?;
                    self.typedefs
                        .insert(alias.clone(), Type::Pointer(Pointee::Int));
                    self.function_pointer_typedefs.insert(alias);
                    return Ok(());
                }
                let name = self.parse_identifier()?;
                // An array typedef (`typedef float Mtx[3][4];`) — record the element
                // type, total element count (member layout size), and the INNER
                // element count (the product of the dimensions after the first: the
                // row stride a decayed parameter subscripts by; 1 for a 1-D typedef).
                if *self.peek() == Token::BracketOpen {
                    let mut total: u16 = 1;
                    let mut inner: u16 = 1;
                    let mut first = true;
                    while *self.peek() == Token::BracketOpen {
                        self.advance();
                        let count = self.parse_integer_constant()? as u16;
                        self.expect(Token::BracketClose)?;
                        total = total.saturating_mul(count);
                        if !first {
                            inner = inner.saturating_mul(count);
                        }
                        first = false;
                    }
                    self.expect(Token::Semicolon)?;
                    self.array_typedefs.insert(name, (aliased, total, inner));
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
            if matches!(self.peek(), Token::Identifier(word) if word == "union")
                && self.tokens.get(self.position + 2) == Some(&Token::BraceOpen)
            {
                self.advance(); // `union`
                let tag = self.parse_identifier()?;
                let layout = self.parse_union_body()?;
                self.structs.insert(tag, layout);
                if *self.peek() == Token::Semicolon {
                    self.advance();
                    return Ok(());
                }
                return Err(Diagnostic::error(
                    "a union-definition global value is not supported yet (roadmap)",
                ));
            }
            if *self.peek() == Token::KeywordStruct
                && self.tokens.get(self.position + 2) == Some(&Token::BraceOpen)
            {
                self.expect(Token::KeywordStruct)?;
                let tag = self.parse_identifier()?;
                let layout = self.parse_struct_body()?;
                self.structs.insert(tag.clone(), layout);
                if *self.peek() == Token::Semicolon {
                    self.advance();
                    return Ok(());
                }
                let struct_type = self.struct_value_type(&tag).ok_or_else(|| {
                    Diagnostic::error(format!("struct '{tag}' value layout is not declared"))
                })?;
                loop {
                    let name = self.parse_identifier()?;
                    // Only a scalar, uninitialized struct global is in the subset; an
                    // array or initializer defers honestly (no miscompile).
                    if !matches!(self.peek(), Token::Semicolon | Token::Comma) {
                        return Err(Diagnostic::error("an initialized or array struct-definition global is not supported yet (roadmap)"));
                    }
                    self.variable_structs.insert(name.clone(), tag.clone());
                    globals.push(GlobalDeclaration {
                        is_weak: false,
                        non_static_functions_before: functions
                            .iter()
                            .filter(|function| !function.is_static)
                            .count(),
                        functions_before: functions.len(),
                        declared_type: struct_type,
                        name,
                        is_extern,
                        is_static,
                        array_length: None,
                        array_length_inferred: false,
                        initializer: None,
                        is_const: false,
                        address_initializer: None,
                        data_bytes: None,
                        data_relocations: Vec::new(),
                        section: declspec_section.clone(),
                        attribute_alignment: None,
                    });
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
            // Keep the declared aggregate identity before parsing attributes, placement
            // expressions, or initializers: each may contain a cast whose own parse_type call
            // overwrites `last_struct_tag` (notably `T hw : (u32)(void*)ADDRESS`).
            let declared_struct_tag = self.last_struct_tag.clone();
            // An array-typedef type (`Mtx g;`): parse_type returned the DECAYED pointer
            // (right for a function's return type) and left `(element, total, inner)`
            // in the marker — the GLOBAL branch below declares the real array object
            // from it (a row-pointer typedef reports total == 0 and stays a pointer).
            let array_typedef_marker = self.last_array_typedef.take();
            // A struct-POINTER return type carries a tag (`struct S *get(...)`); capture it now
            // (before later declarators overwrite `last_struct_tag`) so a function declarator below
            // can record it for `get()->field` resolution. A struct-VALUE return is not recorded
            // (its `get().field` needs the unmodeled struct-return ABI and stays deferred).
            let return_struct_tag = if matches!(return_type, Type::StructPointer { .. }) {
                declared_struct_tag.clone()
            } else {
                None
            };
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
                self.locations.remove(self.position);
                self.tokens.remove(self.position + 1); // `)` (the name shifted down)
                self.locations.remove(self.position + 1);
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
                let mut pointer_array_unsized = false;
                if self.eat_keyword(Token::BracketOpen) {
                    if let Token::IntegerLiteral(count) = self.peek() {
                        pointer_array_length = Some(*count as u16);
                        self.advance();
                    } else {
                        pointer_array_unsized = true;
                    }
                    self.expect(Token::BracketClose)?;
                }
                self.expect(Token::ParenClose)?;
                self.expect(Token::ParenOpen)?;
                let mut depth = 1;
                while depth > 0 {
                    match self.advance() {
                        Token::ParenOpen => depth += 1,
                        Token::ParenClose => depth -= 1,
                        Token::EndOfFile => {
                            return Err(Diagnostic::error(
                                "unterminated function-pointer declarator",
                            ))
                        }
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
                // An UNSIZED fp-array (`void (*tbl[])(void)`) infers its length from
                // the initializer list (`= { e1, e2 }` — item.c's itemFuncTbl); with
                // neither an initializer nor `extern` there is nothing to infer.
                if pointer_array_unsized {
                    match address_initializer.as_ref() {
                        Some(elements) => pointer_array_length = Some(elements.len() as u16),
                        // An extern unsized fp-array (`extern void (*_dtors[])(void);`)
                        // keeps its pre-existing None length (abort_exit.c is byte-exact
                        // with it — do not disturb).
                        None if is_extern => {}
                        None => {
                            return Err(Diagnostic::error(
                                "a function-pointer array needs an explicit length (roadmap)",
                            ))
                        }
                    }
                }
                globals.push(GlobalDeclaration {
                    is_weak: false,
                    non_static_functions_before: functions
                        .iter()
                        .filter(|function| !function.is_static)
                        .count(),
                    functions_before: functions.len(),
                    declared_type: Type::StructPointer { element_size: 0 },
                    name: pointer_name,
                    is_extern,
                    is_static,
                    array_length: pointer_array_length,
                    array_length_inferred: pointer_array_unsized,
                    initializer: None,
                    is_const: false,
                    address_initializer,
                    data_bytes: None,
                    data_relocations: Vec::new(),
                    section: declspec_section.clone(),
                    attribute_alignment: None,
                });
                return Ok(());
            }
            let mut name = self.parse_identifier()?;
            // An out-of-class C++ member definition spells its declarator as
            // `Return Class::method(args)`. Keep qualification separate from
            // ordinary identifiers: the ELF symbol is CodeWarrior-mangled and
            // the ABI carries an implicit `this` parameter in r3.
            let qualified_scope = if *self.peek() == Token::Colon && *self.peek_at(1) == Token::Colon {
                let mut scopes = Vec::new();
                loop {
                    scopes.push(name);
                    self.advance();
                    self.advance();
                    name = self.parse_identifier()?;
                    if *self.peek() != Token::Colon || *self.peek_at(1) != Token::Colon {
                        break;
                    }
                }
                Some(scopes.join("::"))
            } else {
                None
            };
            let namespace_scope = qualified_scope
                .as_ref()
                .filter(|scope| self.cxx_namespaces.contains(scope.as_str()))
                .cloned();
            let member_scope = qualified_scope.filter(|_| namespace_scope.is_none());
            let member_layout_scope = member_scope
                .as_deref()
                .and_then(|scope| scope.rsplit("::").next())
                .map(str::to_string);
            // A `__attribute__((aligned(n)))` immediately AFTER the declarator name
            // (`T x ATTRIBUTE_ALIGN(n);` — the scalar form). Consuming it here makes the
            // following token the real `;`/`[`/`=`, so the global-variable branch below is
            // entered (otherwise the stray `__attribute__` falls to the function path and
            // the declaration is skipped — a missing-symbol DIFF). `None` when absent.
            let attribute_alignment_name = self.skip_attributes()?;
            if let Some(scope) = &member_scope {
                if *self.peek() != Token::ParenOpen {
                    name = self.mangle_data_member_in_current_namespace(scope, &name)?;
                }
            }
            // A `(` after the name begins a FUNCTION declarator: record a struct-pointer
            // return tag so `name()->field` resolves the returned pointee's layout.
            if *self.peek() == Token::ParenOpen {
                if let Some(tag) = &return_struct_tag {
                    self.function_return_structs
                        .insert(name.clone(), tag.clone());
                }
            }
            // `type name [N]… : addr;` — a FIXED-ADDRESS global (mwcc's `AT_ADDRESS(a)` = `: (a)`; the
            // hardware-register pattern: the GX FIFO `volatile PPCWGPipe GXWGFifo : 0xCC008000`, or an
            // array `vu32 __EXIRegs[16] : 0xCC006800`). Look past the name and any `[N]` brackets for
            // the `:` that marks a placement. A scalar/aggregate is recorded for desugaring to a const-
            // address deref; an ARRAY is recorded separately (kept a variable — its subscript compiles
            // to mwcc's array form, not a cast fold) and handed to codegen.
            let is_fixed_address = {
                let mut scan = self.position;
                while matches!(self.tokens.get(scan), Some(Token::BracketOpen)) {
                    while !matches!(self.tokens.get(scan), Some(Token::BracketClose) | None) {
                        scan += 1;
                    }
                    scan += 1; // past `]`
                }
                matches!(self.tokens.get(scan), Some(Token::Colon))
            };
            if is_fixed_address {
                // A fixed-address declaration through an array typedef would record the
                // decayed pointer as the element type (wrong stride) — defer.
                if array_typedef_marker.is_some() {
                    return Err(Diagnostic::error(
                        "a fixed-address array-typedef global is not supported yet (roadmap)",
                    ));
                }
                let mut is_array = false;
                while *self.peek() == Token::BracketOpen {
                    is_array = true;
                    self.advance();
                    let _length = self.parse_integer_constant()?;
                    self.expect(Token::BracketClose)?;
                }
                self.expect(Token::Colon)?;
                let address = if *self.peek() == Token::ParenOpen {
                    self.advance();
                    let value = self.parse_integer_constant()?;
                    self.expect(Token::ParenClose)?;
                    value
                } else {
                    self.parse_integer_constant()?
                };
                self.expect(Token::Semicolon)?;
                if is_array {
                    self.fixed_address_arrays
                        .insert(name.clone(), (address, return_type));
                } else {
                    let tag = declared_struct_tag.clone();
                    // An aggregate casts to a struct pointer (member access via the const-address
                    // member path); a scalar casts to a pointer of its own pointee (direct load/store).
                    // An unsupported scalar type is not recorded — it defers.
                    let cast_target = match &tag {
                        Some(tag) => {
                            let size = self
                                .structs
                                .get(tag)
                                .map(|layout| layout.size)
                                .unwrap_or_else(|| type_size(return_type));
                            Some(Type::StructPointer { element_size: size })
                        }
                        None => pointee_of(return_type).ok().map(Type::Pointer),
                    };
                    if let Some(cast_target) = cast_target {
                        self.fixed_address_globals
                            .insert(name.clone(), (address, cast_target, tag));
                    }
                }
                return Ok(());
            }
            // `type name;`, `type name[N];`, or comma-separated declarators is a
            // global variable declaration. A `(` instead begins a function. (An
            // initialized global `type name = …;` is not in the subset yet and
            // falls through to the function path, which reports it.)
            if matches!(
                self.peek(),
                Token::Semicolon | Token::Comma | Token::BracketOpen | Token::Equals
            ) {
                // An array-typedef global (`Mtx g;`) is the whole ARRAY object — as if
                // `float g[12];` had been written: the declared type becomes the element
                // and the typedef's total element count seeds the dimensions (explicit
                // brackets multiply it: `Mtx pool[2]` is 24 elements). A row-pointer
                // typedef (total == 0) keeps the pointer type — the object IS a pointer.
                let mut return_type = return_type;
                let mut array_typedef_length: Option<u16> = None;
                if let Some((element, total, _inner)) = array_typedef_marker {
                    if total > 0 {
                        return_type = element;
                        array_typedef_length = Some(total);
                    }
                }
                // A `const` file-scope global lands in a *read-only* section
                // (`.sdata2` if small, `.rodata` if large). Record it; the lowering
                // routes the supported shapes and defers the rest. `parse_type` set
                // this for the declared type and nothing since has reset it.
                let is_const = self.last_type_was_const;
                let pointer_object_const = self.last_pointer_const;
                // A struct-typed global (pointer, value, or array) carries the struct
                // tag `parse_type` stashed, so `gp->field` / `g.field` / `arr[i].field`
                // resolve the member layout. Codegen handles the struct-pointer base
                // and defers the value/array bases (no miscompile).
                let global_struct_tag = declared_struct_tag.clone();
                if let Some(tag) = &global_struct_tag {
                    self.global_structs.insert(name.clone(), tag.clone());
                }
                let mut declarator_name = name.clone();
                loop {
                    // Array dimensions `[A][B]…`: each `[N]` is an explicit length,
                    // `[]` (only the first dimension) is inferred from the
                    // initializer; no brackets is a scalar. A multi-dimensional array
                    // flattens row-major to one element list of the dimensions' product.
                    let mut dimensions: Vec<Option<u16>> = Vec::new();
                    if let Some(total) = array_typedef_length {
                        dimensions.push(Some(total));
                    }
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
                    // A `__attribute__((aligned(n)))` AFTER the dimensions (`u8 buf[32]
                    // ATTRIBUTE_ALIGN(32);` — the common dolphin DMA-buffer form). Combine
                    // with any post-name attribute; the larger requested alignment wins.
                    let attribute_alignment_dims = self.skip_attributes()?;
                    let attribute_alignment =
                        match (attribute_alignment_name, attribute_alignment_dims) {
                            (Some(a), Some(b)) => Some(a.max(b)),
                            (a, b) => a.or(b),
                        };
                    // A pointer global initialized with addresses (`int *p = &g;` or
                    // a `{&a, &b}` array) is a set of data relocations, not constants.
                    // An array of word-field structs with a pointer field (a
                    // `{ "name", id }` table) flattens to the same address-initializer
                    // (pointer slots relocate, scalar slots are literal bytes).
                    let table_fields =
                        if !dimensions.is_empty() && matches!(return_type, Type::Struct { .. }) {
                            global_struct_tag
                                .as_deref()
                                .and_then(|tag| self.struct_pointer_table_fields(tag))
                        } else {
                            None
                        };
                    let mut address_initializer = None;
                    let mut initializer = None;
                    let mut data_relocations: Vec<(u32, String, i32)> = Vec::new();
                    let mut data_bytes: Option<Vec<u8>> = None;
                    if matches!(return_type, Type::Pointer(_) | Type::StructPointer { .. })
                        && *self.peek() == Token::Equals
                    {
                        self.advance();
                        address_initializer = Some(self.parse_address_initializer()?);
                    } else if table_fields.is_some() && *self.peek() == Token::Equals {
                        self.advance();
                        address_initializer =
                            Some(self.parse_struct_pointer_table(table_fields.as_ref().unwrap())?);
                    } else if matches!(return_type, Type::Struct { .. })
                        && global_struct_tag.is_some()
                        && *self.peek() == Token::Equals
                    {
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
                        // Non-constant float elements zero-fill the image and
                        // become startup assignments in the synthesized
                        // `__sinit_ctx_c` (sunshine trigf's __four_over_pi_m1).
                        for (index, expression) in self.initializer_pending.drain(..) {
                            self.pending_sinit.push((name.clone(), index, expression));
                        }
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
                    let array_length_inferred = dimensions.iter().any(Option::is_none);
                    let array_length = if dimensions.is_empty() {
                        None
                    } else if let Some(explicit) =
                        dimensions.iter().copied().collect::<Option<Vec<u16>>>()
                    {
                        // Every dimension is explicit: the length is their product.
                        Some(
                            explicit
                                .iter()
                                .map(|&dimension| dimension as u32)
                                .product::<u32>() as u16,
                        )
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
                        match initializer
                            .as_ref()
                            .map(Vec::len)
                            .or(address_initializer.as_ref().map(Vec::len))
                        {
                            Some(length) => Some(length as u16),
                            // `extern T name[];` — an UNSIZED extern array (Runtime's
                            // `extern __eti_init_info _eti_init_info[];`). Its size is
                            // unknowable here, and it only feeds the SDA21-vs-ADDR16
                            // total-size <= 8 choice — mwcc addresses an unknown-size
                            // array absolutely (lis/addi, measured), so register it with
                            // a huge sentinel length. No data is emitted for an extern.
                            None if is_extern => Some(u16::MAX),
                            None => {
                                return Err(Diagnostic::error(
                                    "an array with no length needs an initializer",
                                ))
                            }
                        }
                    };
                    if let Some(tag) = &global_struct_tag {
                        self.variable_structs
                            .insert(declarator_name.clone(), tag.clone());
                    }
                    // mwcc INLINES a `const` scalar-int global's value at each read (`return g` ->
                    // `li r3,VALUE`) while still emitting g's read-only `.sdata2` storage. Fold reads
                    // like an enum constant; the global is still pushed below so the writer emits the
                    // storage. A narrow const reads as its value EXTENDED to int per its signedness
                    // (`const char c=200` reads -56; `const unsigned char=200` reads 200) while the
                    // storage keeps the raw byte — so fold the value reduced to the declared width.
                    // (extern has no initializer; `&g` then folds to AddressOf{literal} and defers —
                    // safe, not a wrong load.)
                    if is_const
                        && !is_extern
                        && dimensions.is_empty()
                        && matches!(
                            return_type,
                            Type::Int
                                | Type::UnsignedInt
                                | Type::Char
                                | Type::UnsignedChar
                                | Type::Short
                                | Type::UnsignedShort
                        )
                        && initializer
                            .as_ref()
                            .map_or(false, |values| values.len() == 1)
                    {
                        let folded = crate::expressions::truncate_to_integer(
                            initializer.as_ref().unwrap()[0],
                            return_type,
                        );
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
                    self.global_sizes
                        .insert(declarator_name.clone(), (total_bytes, array_element));
                    self.global_types
                        .insert(declarator_name.clone(), return_type);
                    // For a POINTER declarator, a LEADING `const` binds the
                    // POINTEE (`const char* dummy = "C"` is a WRITABLE pointer
                    // in `.sdata` — measured: locale) — the object itself is
                    // not const.
                    // A pointer global is object-const only when the `const` TRAILS the
                    // star (`void* const`); a leading `const void*` is pointee-const and
                    // stays writable. A non-pointer keeps the plain leading-const rule.
                    let object_is_const =
                        if matches!(return_type, Type::Pointer(_) | Type::StructPointer { .. }) {
                            pointer_object_const
                        } else {
                            is_const
                        };
                    globals.push(GlobalDeclaration {
                        is_weak: false,
                        non_static_functions_before: functions
                            .iter()
                            .filter(|function| !function.is_static)
                            .count(),
                        functions_before: functions.len(),
                        declared_type: return_type,
                        name: declarator_name,
                        is_extern,
                        is_static,
                        array_length,
                        array_length_inferred,
                        initializer,
                        is_const: object_is_const,
                        address_initializer,
                        data_bytes,
                        data_relocations: std::mem::take(&mut data_relocations),
                        section: declspec_section.clone(),
                        attribute_alignment,
                    });
                    if *self.peek() == Token::Comma {
                        self.advance();
                        // A later pointer declarator carries its own `*` (`int *a, *b;`): the base type
                        // is already the pointer type formed by the first declarator, so consume the `*`
                        // and reuse it. A MIXED list (`int *a, b;`) or a MULTI-LEVEL one (`int *a, **b;`)
                        // needs a per-declarator type, so defer rather than mis-type a declarator.
                        if *self.peek() == Token::Star {
                            if !matches!(return_type, Type::Pointer(_) | Type::StructPointer { .. })
                            {
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
            let mut cxx_parameters = Vec::new();
            let mut is_variadic = false;
            // Row-stride records are scoped to ONE function's parameters — a stale
            // entry from a previous function would mis-stride a same-named variable.
            self.decayed_row_pointers.clear();
            // `(void)` is an empty parameter list — but only when the `void` is the
            // whole list; `void *p` / `void (*f)()` are real first parameters.
            if *self.peek() == Token::KeywordVoid
                && self.tokens.get(self.position + 1) == Some(&Token::ParenClose)
            {
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
                    let mut parameter_type = self.parse_type()?;
                    let cxx_source_type = parameter_type;
                    let cxx_is_wchar = self.last_type_was_wchar;
                    let cxx_source_is_aggregate_value =
                        self.last_type_was_aggregate_reference;
                    let cxx_pointee_const = self.last_type_was_const;
                    let cxx_pointer_const = self.last_pointer_const;
                    let cxx_pointer_depth = self.last_cxx_pointer_depth;
                    let cxx_pointer_base = self.last_cxx_pointer_base;
                    // An array-typedef (`Mtx m`) or row-pointer-typedef (`MtxPtr m`)
                    // parameter: the type already decayed to the element pointer;
                    // keep `(element, inner)` to record the row stride under the
                    // parameter's name below so `m[i][j]` desugars with it.
                    let array_typedef_marker = self.last_array_typedef.take();
                    // Extra declarator stars — `wchar_t ** end` is a pointer to
                    // pointer; each further `*` deepens to a plain pointer.
                    while *self.peek() == Token::Star {
                        self.advance();
                        if array_typedef_marker.is_some() {
                            return Err(Diagnostic::error("a pointer to an array-typedef parameter is not supported yet (roadmap)"));
                        }
                        parameter_type = Type::Pointer(Pointee::Pointer);
                    }
                    let struct_tag = self.last_struct_tag.take();
                    let enum_tag = self.last_enum_tag.take();
                    let cxx_qualified_name = enum_tag.or_else(|| {
                        struct_tag.as_ref().map(|tag| {
                            self.struct_typedefs
                                .get(tag)
                                .cloned()
                                .unwrap_or_else(|| tag.clone())
                        })
                    });
                    let is_reference = self.eat_keyword(Token::Ampersand);
                    if is_reference {
                        // References use a word-sized address in the EABI, while
                        // their source identity remains in `cxx_parameters` for
                        // CodeWarrior name mangling.
                        parameter_type = Type::StructPointer { element_size: 0 };
                    }
                    // A function-pointer parameter `RET (*name)(params)` is a 4-byte
                    // opaque pointer; consume its declarator and signature.
                    if *self.peek() == Token::ParenOpen
                        && self.tokens.get(self.position + 1) == Some(&Token::Star)
                    {
                        self.advance(); // `(`
                        self.advance(); // `*`
                        let name = if matches!(self.peek(), Token::Identifier(_)) {
                            self.parse_identifier()?
                        } else {
                            String::new()
                        };
                        self.expect(Token::ParenClose)?;
                        self.expect(Token::ParenOpen)?;
                        let mut depth = 1;
                        while depth > 0 {
                            match self.advance() {
                                Token::ParenOpen => depth += 1,
                                Token::ParenClose => depth -= 1,
                                Token::EndOfFile => {
                                    return Err(Diagnostic::error(
                                        "unterminated function-pointer parameter",
                                    ))
                                }
                                _ => {}
                            }
                        }
                        parameters.push(Parameter {
                            parameter_type: Type::StructPointer { element_size: 0 },
                            name,
                        });
                        cxx_parameters.push(crate::cxx::CxxParameterType::plain(
                            Type::StructPointer { element_size: 0 },
                        ));
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
                            if array_typedef_marker.is_some() {
                                // `Mtx m[N]` decays to a pointer to the WHOLE array — not modeled.
                                return Err(Diagnostic::error("an array of an array-typedef parameter is not supported yet (roadmap)"));
                            }
                            self.advance(); // `[`
                            while !matches!(self.peek(), Token::BracketClose | Token::EndOfFile) {
                                self.advance(); // skip the optional size expression
                            }
                            self.expect(Token::BracketClose)?;
                            match parameter_type {
                                Type::Struct { size, .. } => {
                                    Type::StructPointer { element_size: size }
                                }
                                scalar => Type::Pointer(pointee_of(scalar)?),
                            }
                        } else {
                            parameter_type
                        };
                        if let Some(tag) = &struct_tag {
                            if !name.is_empty() {
                                self.variable_structs.insert(name.clone(), tag.clone());
                            }
                        }
                        // Record the row stride for a decayed array-typedef / row-pointer
                        // parameter so `m[i][j]` desugars to the strided Member access.
                        if let Some((element, _total, inner)) = array_typedef_marker {
                            if !name.is_empty() {
                                let stride = inner.max(1) as u32 * (element.width() as u32 / 8);
                                self.decayed_row_pointers
                                    .insert(name.clone(), (element, stride as u16));
                            }
                        }
                        parameters.push(Parameter {
                            parameter_type,
                            name,
                        });
                        cxx_parameters.push(crate::cxx::CxxParameterType::parsed(
                            cxx_source_type,
                            cxx_qualified_name,
                            cxx_is_wchar,
                            is_reference,
                            cxx_source_is_aggregate_value,
                            cxx_pointee_const,
                            cxx_pointer_const,
                        ).with_pointer_shape(cxx_pointer_depth, cxx_pointer_base));
                    }
                    if *self.peek() == Token::Comma {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
            self.expect(Token::ParenClose)?;
            let mut member_is_const = false;
            if member_scope.is_some() {
                while matches!(self.peek(), Token::Identifier(word)
                    if matches!(word.as_str(), "const" | "volatile" | "override" | "final"))
                {
                    if matches!(self.peek(), Token::Identifier(word) if word == "const") {
                        member_is_const = true;
                    }
                    self.advance();
                }
            }

            // Keep this source fact before C++ member lowering inserts the
            // implicit `this` parameter. Later 4.x compilers charge one
            // anonymous ordinal for each name written on a prototype, even
            // though the callable type only retains parameter types.
            let source_named_parameter_count = parameters
                .iter()
                .filter(|parameter| !parameter.name.is_empty())
                .count();

            let constructor_scope = member_layout_scope
                .as_ref()
                .filter(|scope| scope.as_str() == name.as_str())
                .cloned();
            let destructor_scope = member_layout_scope
                .as_ref()
                .filter(|_| name == "__dt")
                .cloned();
            let constructor_initializers = if let Some(scope) = &constructor_scope {
                self.parse_constructor_initializers(scope)?
            } else {
                Vec::new()
            };

            if let Some(scope) = &member_scope {
                let source_name = if constructor_scope.is_some() {
                    "__ct"
                } else {
                    &name
                };
                name = if member_is_const {
                    self.mangle_typed_const_member_in_current_namespace(
                        scope,
                        source_name,
                        &cxx_parameters,
                    )?
                } else {
                    self.mangle_typed_member_in_current_namespace(
                        scope,
                        source_name,
                        &cxx_parameters,
                    )?
                };
                parameters.insert(
                    0,
                    Parameter {
                        parameter_type: Type::StructPointer {
                            element_size: member_layout_scope
                                .as_deref()
                                .and_then(|layout_scope| self.structs.get(layout_scope))
                                .map_or(0, |layout| layout.size),
                        },
                        name: "this".to_string(),
                    },
                );
                // A virtual destructor has an ABI-only signed-short deleting
                // flag in r4. It is deliberately absent from the mangled type,
                // but must be present in executable IR so the ordinary
                // conditional/call lowering can generate the deleting form.
                if destructor_scope
                    .as_deref()
                    .and_then(|layout_scope| self.cxx_classes.get(layout_scope))
                    .is_some_and(|class| class.has_virtual_destructor)
                {
                    parameters.push(Parameter {
                        parameter_type: Type::Short,
                        name: "__destroy".to_string(),
                    });
                }
            } else if let Some(scope) = &namespace_scope {
                let source_name = name.clone();
                name = self.mangle_typed_free_function_in_scope(
                    scope,
                    &source_name,
                    &cxx_parameters,
                    is_variadic,
                )?;
                self.register_qualified_free_cxx_function(
                    scope,
                    &source_name,
                    &name,
                    &parameters
                        .iter()
                        .map(|parameter| parameter.parameter_type)
                        .collect::<Vec<_>>(),
                    is_variadic,
                );
                if let Some(tag) = &return_struct_tag {
                    self.function_return_structs.insert(name.clone(), tag.clone());
                }
            } else if self.cplusplus && name != "main" {
                let source_name = name.clone();
                name = self.mangle_typed_free_function(
                    &source_name,
                    &cxx_parameters,
                    is_variadic,
                )?;
                self.register_free_cxx_function(
                    &source_name,
                    &name,
                    &parameters
                        .iter()
                        .map(|parameter| parameter.parameter_type)
                        .collect::<Vec<_>>(),
                    is_variadic,
                );
                if let Some(tag) = &return_struct_tag {
                    self.function_return_structs.insert(name.clone(), tag.clone());
                }
            }

            if *self.peek() == Token::Semicolon {
                self.advance(); // a prototype — record its return + parameter types, keep looking
                self.named_prototype_parameters += source_named_parameter_count;
                let parameter_types = parameters
                    .iter()
                    .map(|parameter| parameter.parameter_type)
                    .collect();
                if is_weak {
                    self.weak_functions.insert(name.clone());
                }
                if is_static {
                    self.static_functions.insert(name.clone());
                }
                if let Some(section) = &declspec_section {
                    if self
                        .section_functions
                        .insert(name.clone(), section.clone())
                        .is_none()
                    {
                        self.section_prototype_order.push(name.clone());
                    }
                }
                if is_variadic {
                    self.variadic_definitions.insert(name.clone());
                }
                prototypes.push((name, return_type, parameter_types));
                return Ok(());
            }
            // A variadic function DEFINITION parses like any other (so a capture
            // can hash-match it); its name goes in a SIDE set — never in the
            // hashed Function — and the general lowering defers it (the
            // variadic-register-save prologue is capture-only for now).
            if is_variadic {
                self.variadic_definitions.insert(name.clone());
            }
            // A `static inline` DEFINITION is normally skipped-and-inlined (the
            // mp4 shape — the error routes it to the skip machinery). But with a
            // PRIOR PROTOTYPE the call sites precede the body, so mwcc cannot
            // inline it: it MATERIALIZES out-of-line as a local function at the
            // definition's source position (measured: AC/ww/sunshine uart).
            let mut materialize_by_calls = false;
            if is_inline {
                // Referenced EARLIER (a prototype, or a call already parsed into a
                // previous function — uart_8's IMPLICIT-declaration shape) means the
                // call sites precede the body: mwcc cannot inline and MATERIALIZES.
                let name_set: std::collections::HashSet<String> =
                    std::iter::once(name.clone()).collect();
                let had_prototype = prototypes
                    .iter()
                    .any(|(prototype_name, _, _)| *prototype_name == name);
                let had_call = functions.iter().any(|earlier| {
                    earlier
                        .statements
                        .iter()
                        .any(|statement| statement_calls(statement, &name_set))
                        || earlier
                            .guards
                            .iter()
                            .any(|guard| expression_calls(&guard.condition, &name_set))
                        || earlier
                            .return_expression
                            .as_ref()
                            .is_some_and(|expression| expression_calls(expression, &name_set))
                });
                // The trigger is a CALL compiled before the definition — a
                // prototype alone does NOT materialize (p2's wctomb: prototyped,
                // defined, THEN called — mwcc inlines it at the later call).
                // A static inline mwcc declines to inline MATERIALIZES even
                // with no earlier call: measured on ww alloc.c, the trigger is
                // TWO-PLUS calls to real (non-skipped-inline) functions in the
                // body (dealloc_var: Block_link+__sys_free; __pool_free:
                // dealloc_fixed+dealloc_var). It emits AFTER the next real
                // function (the deferred-materialization queue).
                // TU-local functions seen so far: DEFINITIONS and already-skipped
                // inlines only. Merely-PROTOTYPED names (extern OSReport in the
                // dolphin headers) are EXTERNAL — excluded, so a heap-init inline
                // calling only OS* helpers (GCN InitDefaultHeap) stays inlinable.
                let mut tu_local: std::collections::HashSet<String> =
                    self.skipped_inline_names.clone();
                for function in functions.iter() {
                    tu_local.insert(function.name.clone());
                }
                materialize_by_calls =
                    !had_call && is_static && self.body_local_statement_call_count(&tu_local) >= 2;
                if !had_call && !materialize_by_calls {
                    return Err(Diagnostic::error(
                        "an inline function definition is skipped (inlined at call sites)",
                    ));
                }
                self.materialized_inline_candidates.push(name.clone());
                if is_static {
                    // Implicit-declaration materialization (no prototype): the call
                    // relocations bind the surviving UND ghost, and the local FUNC
                    // symbol trails its own static locals (measured: ww uart).
                    // A materialize-by-CALLS function instead binds its local
                    // FUNC directly (measured: ww alloc — no UND ghost).
                    if !had_prototype && !materialize_by_calls {
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
            let function_is_static = is_static || self.static_functions.contains(&name);
            // The section may sit on the definition (mp4) or on an earlier prototype
            // (pikmin's DECL_SECT on the memcpy proto) — prefer the definition's.
            let proto_section = self.section_functions.get(&name).cloned();
            if self.defer_codegen {
                self.deferred_function_names.push(name.clone());
            }
            let previous_member_scope = self.current_member_scope.clone();
            let previous_this_struct = self.variable_structs.get("this").cloned();
            if let Some(scope) = &member_layout_scope {
                self.current_member_scope = Some(scope.clone());
                self.variable_structs
                    .insert("this".to_string(), scope.clone());
            }
            let parsed_function = self.function_body(
                if let Some(scope) = &constructor_scope {
                    Type::StructPointer {
                        element_size: self.structs.get(scope).map_or(0, |layout| layout.size),
                    }
                } else {
                    return_type
                },
                name,
                function_is_static,
                parameters,
            );
            self.current_member_scope = previous_member_scope;
            match previous_this_struct {
                Some(scope) => {
                    self.variable_structs.insert("this".to_string(), scope);
                }
                None => {
                    self.variable_structs.remove("this");
                }
            }
            let mut function = parsed_function?;
            if !constructor_initializers.is_empty() {
                function.statements.splice(0..0, constructor_initializers);
            }
            let special_member_scope = constructor_scope.as_ref().or(destructor_scope.as_ref());
            if let Some(scope) = special_member_scope {
                if let Some(class) = self.cxx_classes.get(scope) {
                    if let Some(vptr_offset) = class.vptr_offset {
                        let vtable = format!("__vt__{}{}", scope.len(), scope);
                        let target = Expression::Member {
                            base: Box::new(Expression::Variable("this".to_string())),
                            offset: vptr_offset,
                            member_type: Type::UnsignedInt,
                            index_stride: None,
                        };
                        let vptr_store = Statement::Store {
                            target,
                            value: Expression::AddressOf {
                                operand: Box::new(Expression::Variable(vtable.clone())),
                            },
                        };
                        if destructor_scope.is_some() && class.has_virtual_destructor {
                            function.return_type = Type::StructPointer {
                                element_size: self.structs.get(scope).map_or(0, |layout| layout.size),
                            };
                            function.statements = vec![Statement::If {
                                condition: Expression::Variable("this".to_string()),
                                then_body: vec![
                                    vptr_store,
                                    Statement::If {
                                        condition: Expression::Binary {
                                            operator: mwcc_syntax_trees::BinaryOperator::Greater,
                                            left: Box::new(Expression::Variable("__destroy".to_string())),
                                            right: Box::new(Expression::IntegerLiteral(0)),
                                        },
                                        then_body: vec![Statement::Expression(Expression::Call {
                                            name: "__dl__FPv".to_string(),
                                            arguments: vec![Expression::Variable("this".to_string())],
                                        })],
                                        else_body: Vec::new(),
                                    },
                                ],
                                else_body: Vec::new(),
                            }];
                            function.return_expression =
                                Some(Expression::Variable("this".to_string()));

                            if !globals.iter().any(|global| global.name == vtable) {
                                let table_size = 8 + class.virtual_slots.max(1) * 4;
                                globals.push(GlobalDeclaration {
                                    declared_type: Type::Struct {
                                        size: table_size as u32,
                                        align: 4,
                                    },
                                    name: vtable,
                                    is_extern: false,
                                    is_static: false,
                                    is_weak: false,
                                    non_static_functions_before: functions
                                        .iter()
                                        .filter(|function| !function.is_static)
                                        .count(),
                                    functions_before: functions.len(),
                                    array_length: None,
                                    array_length_inferred: false,
                                    initializer: None,
                                    is_const: false,
                                    address_initializer: None,
                                    data_bytes: Some(vec![0; table_size]),
                                    data_relocations: vec![(8, function.name.clone(), 0)],
                                    section: None,
                                    attribute_alignment: None,
                                });
                            }
                        } else if constructor_scope.is_some() {
                            function.statements.insert(0, vptr_store);
                        }
                    }
                }
            }
            if constructor_scope.is_some() && function.return_expression.is_none() {
                function.return_expression = Some(Expression::Variable("this".to_string()));
            }
            function.is_weak = function_is_weak;
            function.section = declspec_section.clone().or(proto_section);
            function.text_deferred = materialize_by_calls;
            functions.push(function);
        }
        Ok(())
    }

    /// Count STATEMENT-LEVEL call sites in the definition body at the cursor
    /// (`name ( … ) ;` — a bare, side-effecting call) that target a TU-LOCAL
    /// function (in `local_names`: a definition, prototype, or skipped inline).
    /// mwcc's inline-cost heuristic bails on a body with two-plus such calls,
    /// MATERIALIZING it out-of-line (measured: ww alloc's dealloc_var calls
    /// Block_link+__unlink; __pool_free calls dealloc_fixed+dealloc_var). Calls
    /// to EXTERNALS (GCN InitDefaultHeap's OSReport/OSGetArenaLo) and
    /// expression-buried calls (fpclassify's `__HI(x)`, `return f()`) do NOT
    /// count — those stay inlinable. Pure lookahead.
    pub(crate) fn body_local_statement_call_count(
        &self,
        local_names: &std::collections::HashSet<String>,
    ) -> usize {
        let mut index = self.position;
        // Find the body's opening brace.
        while !matches!(
            self.tokens.get(index),
            Some(Token::BraceOpen) | Some(Token::EndOfFile) | None
        ) {
            index += 1;
        }
        let mut braces = 0i32;
        let mut count = 0usize;
        loop {
            match self.tokens.get(index) {
                Some(Token::BraceOpen) => braces += 1,
                Some(Token::BraceClose) => {
                    braces -= 1;
                    if braces == 0 {
                        break;
                    }
                }
                Some(Token::Identifier(word))
                    if matches!(self.tokens.get(index + 1), Some(Token::ParenOpen))
                        && local_names.contains(word)
                        && !matches!(word.as_str(), "sizeof" | "if" | "while" | "for" | "switch" | "return")
                        // A BARE statement call starts at a statement boundary:
                        // the preceding token is `;`/`{`/`}`/`:` — NOT `return`,
                        // `=`, or an operator (which make it an expression call,
                        // e.g. `return f();` / `x = f();` — mwcc still inlines).
                        && matches!(
                            self.tokens.get(index.wrapping_sub(1)),
                            Some(Token::Semicolon) | Some(Token::BraceOpen) | Some(Token::BraceClose) | Some(Token::Colon)
                        ) =>
                {
                    // A STATEMENT call closes with `) ;` at the same paren depth:
                    // walk to the matching close-paren and check the next token.
                    let mut cursor = index + 1;
                    let mut parens = 0i32;
                    let mut is_statement_call = false;
                    while let Some(token) = self.tokens.get(cursor) {
                        match token {
                            Token::ParenOpen => parens += 1,
                            Token::ParenClose => {
                                parens -= 1;
                                if parens == 0 {
                                    is_statement_call = matches!(
                                        self.tokens.get(cursor + 1),
                                        Some(Token::Semicolon)
                                    );
                                    break;
                                }
                            }
                            Token::Semicolon | Token::BraceOpen | Token::BraceClose => break,
                            _ => {}
                        }
                        cursor += 1;
                    }
                    if is_statement_call {
                        count += 1;
                    }
                }
                Some(Token::EndOfFile) | None => break,
                _ => {}
            }
            index += 1;
        }
        count
    }

    /// Whether the item at the cursor is an initialized data *definition* — a
    /// top-level `= …` initializer before the `;` (e.g. `OvlInfo list[] = {…};`).
    /// Such a definition emits `.data`/`.sdata` bytes; if its initializer is outside
    /// the subset, skipping it would leave an incomplete object (a silent
    /// whole-object DIFF), so it must instead DEFER the unit like a function we
    /// cannot compile. Pure lookahead — consumes nothing.
    pub(crate) fn item_is_initialized_definition(&self) -> bool {
        if self.item_is_primary_template_declaration() {
            return false;
        }
        let mut index = self.position;
        let (mut brace, mut paren, mut bracket) = (0i32, 0i32, 0i32);
        while let Some(token) = self.tokens.get(index) {
            let top_level = brace == 0 && paren == 0 && bracket == 0;
            match token {
                // A typedef never defines data. Neither does a skipped inline
                // function whose operator name contains `=` (`operator *=`):
                // treating that token as an initializer would turn a harmless
                // unused header definition into a whole-unit defer.
                Token::Identifier(word) if word == "inline" || word == "__inline" => return false,
                Token::Identifier(word) if index == self.position && word == "typedef" => {
                    return false
                }
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
    pub(crate) fn item_is_uninitialized_definition(&self) -> bool {
        if self.item_is_skippable_inline_member_definition() {
            return false;
        }
        // Must start with a scalar type keyword: a struct/union/enum, a typedef alias, or an
        // `extern`-led declaration emits no tentative data symbol, so those stay skippable.
        if !matches!(
            self.tokens.get(self.position),
            Some(
                Token::KeywordInt
                    | Token::KeywordChar
                    | Token::KeywordShort
                    | Token::KeywordUnsigned
                    | Token::KeywordFloat
                    | Token::KeywordVoid
            )
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
    /// construct: a STATIC definition has the generation's configured base, a plain one 0; each `if`
    /// adds 2; `else`/`switch`/`case`/`default`/`||`/`&&` add 1; `while` adds
    /// 4, `for` 5; a ternary adds 0. Unmeasured control constructs (`do`,
    /// `goto`) return an Err so the unit defers rather than mis-bump.
    pub(crate) fn skipped_inline_label_bump(&self) -> Compilation<Option<usize>> {
        let mut index = self.position;
        let mut paren_depth = 0i32;
        let mut saw_parameter_list = false;
        let mut saw_inline = self.item_is_skippable_inline_member_definition();
        let mut saw_static = false;
        while let Some(token) = self.tokens.get(index) {
            match token {
                Token::Identifier(word) if word == "typedef" => return Ok(None),
                Token::Identifier(word) if word == "static" => saw_static = true,
                Token::Identifier(word) if word == "inline" || word == "__inline" => {
                    saw_inline = true
                }
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
                    let mut bump = if saw_static {
                        usize::from(self.skipped_static_inline_label_base)
                    } else {
                        0
                    };
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
                            Token::PipePipe | Token::AmpersandAmpersand if condition_depth > 0 => {
                                bump += 1
                            }
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

    pub(crate) fn item_is_function_definition(&self) -> bool {
        // A function template definition does not itself emit a function body;
        // code appears only for an instantiated specialization. Recovery may
        // therefore skip an unused template just like an inline header helper.
        if self.item_is_primary_template_declaration() {
            return false;
        }
        if self.item_is_skippable_inline_member_definition() {
            return false;
        }
        if self.item_is_kr_function_definition() {
            return true;
        }
        let mut index = self.position;
        let mut paren_depth = 0i32;
        let mut saw_parameter_list = false;
        while let Some(token) = self.tokens.get(index) {
            match token {
                // A typedef is never a function definition. An `inline` definition
                // is an SDK header helper mwcc only emits when used — skip it rather
                // than compile it as a standalone symbol.
                Token::Identifier(word)
                    if word == "typedef" || word == "inline" || word == "__inline" =>
                {
                    return false
                }
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

    /// Recognize an old-style K&R function definition:
    /// `int f(a, b) int a; short b; { ... }`. The ordinary definition
    /// lookahead stops at the first parameter-declaration semicolon and used to
    /// misclassify these as skippable declarations, allowing a partial object
    /// with missing `.text` to escape. Lowering K&R parameters is still roadmap;
    /// this detector exists to preserve the byte-exact-or-defer invariant.
    fn item_is_kr_function_definition(&self) -> bool {
        let mut index = self.position;
        while let Some(token) = self.tokens.get(index) {
            match token {
                // A typedef or dropped inline declaration never owns emitted
                // function text. In particular, `typedef int (F)(void);`
                // otherwise resembles the identifier-only parameter list of a
                // K&R definition and can make recovery defer an unrelated TU.
                Token::Identifier(word)
                    if word == "typedef" || word == "inline" || word == "__inline" =>
                {
                    return false
                }
                Token::ParenOpen => break,
                Token::Semicolon | Token::BraceOpen | Token::EndOfFile => return false,
                _ => index += 1,
            }
        }
        if self.tokens.get(index) != Some(&Token::ParenOpen) {
            return false;
        }
        index += 1;
        let mut saw_parameter = false;
        let mut expect_parameter = true;
        loop {
            match self.tokens.get(index) {
                Some(Token::Identifier(_)) if expect_parameter => {
                    saw_parameter = true;
                    expect_parameter = false;
                }
                Some(Token::Comma) if !expect_parameter => expect_parameter = true,
                Some(Token::ParenClose) if saw_parameter && !expect_parameter => {
                    index += 1;
                    break;
                }
                _ => return false,
            }
            index += 1;
        }

        // An immediate semicolon is an old-style declaration (`int f(a,b);`),
        // and another `(` is a parenthesized function typedef/declarator
        // (`typedef int (F)(void);`). A K&R definition has one or more parameter
        // declarations between its identifier list and body.
        if matches!(
            self.tokens.get(index),
            Some(Token::Semicolon | Token::ParenOpen)
        ) {
            return false;
        }

        let mut saw_parameter_declaration = false;
        let (mut paren_depth, mut bracket_depth) = (0i32, 0i32);
        while let Some(token) = self.tokens.get(index) {
            match token {
                Token::ParenOpen => paren_depth += 1,
                Token::ParenClose => paren_depth -= 1,
                Token::BracketOpen => bracket_depth += 1,
                Token::BracketClose => bracket_depth -= 1,
                Token::Semicolon if paren_depth == 0 && bracket_depth == 0 => {
                    saw_parameter_declaration = true;
                }
                Token::BraceOpen if paren_depth == 0 && bracket_depth == 0 => {
                    return saw_parameter_declaration;
                }
                Token::Equals if paren_depth == 0 && bracket_depth == 0 => return false,
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
    pub(crate) fn capture_skipped_typedef(&mut self) {
        if !matches!(self.tokens.get(self.position), Some(Token::Identifier(word)) if word == "typedef")
        {
            return;
        }
        if self.capture_skipped_template_typedef() {
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
                Token::Identifier(word) if brace == 0 && paren == 0 && bracket == 0 => {
                    alias = Some(word.clone())
                }
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

    pub(crate) fn skip_top_level_declaration(&mut self) {
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
    pub(crate) fn brace_wraps_whole_body(&self) -> bool {
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

    pub(crate) fn function_body(
        &mut self,
        return_type: Type,
        name: String,
        is_static: bool,
        parameters: Vec<Parameter>,
    ) -> Compilation<Function> {
        let body_start_line = self.current_location().line;
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
            self.variable_types
                .insert(parameter.name.clone(), parameter.parameter_type);
        }

        // Zero or more local declarations precede the return statement. A
        // statement that begins with a type keyword is a local declaration;
        // `return` ends the body.
        let mut locals = Vec::new();
        // Function-scope typedefs temporarily participate in expression parsing, then are restored
        // when this body closes. Keeping the restoration here prevents a local alias from leaking
        // into a later function in the translation unit.
        let mut local_function_pointer_typedefs: Vec<(String, Option<Type>, bool)> = Vec::new();
        // A local declaration may open with a storage-class keyword: `static` gives the variable
        // static storage (codegen'd like a global, so recorded and deferred for now), while
        // `register`/`auto` are ordinary-automatic hints with no codegen effect. These are
        // `Identifier` tokens, so peek past them before the type test below.
        loop {
            if matches!(self.peek(), Token::Identifier(word) if word == "typedef") {
                let alias = self.parse_local_function_pointer_typedef()?;
                let previous_type = self
                    .typedefs
                    .insert(alias.clone(), Type::Pointer(Pointee::Int));
                let was_function_pointer = self
                    .function_pointer_typedefs
                    .replace(alias.clone())
                    .is_some();
                local_function_pointer_typedefs.push((
                    alias,
                    previous_type,
                    was_function_pointer,
                ));
                continue;
            }
            let mut is_static = false;
            while let Token::Identifier(word) = self.peek() {
                match word.as_str() {
                    "static" => is_static = true,
                    "register" | "auto" => {}
                    _ => break,
                }
                self.advance();
            }
            if !self.peek_is_type() && !self.peek_is_local_array_typedef() {
                break;
            }
            let declared_type = self.parse_type()?;
            // A volatile local's accesses must not be elided or folded (the straight-
            // line/value-tracking paths would, e.g. `volatile int x = 5; return x;` ->
            // `li r3,5` instead of mwcc's store-then-load). Defer until that is modeled.
            if self.last_type_was_volatile {
                return Err(Diagnostic::error(
                    "a volatile local is not supported yet (roadmap)",
                ));
            }
            // An array-typedef local (`Mtx proj;`) is exactly the flat local array
            // `f32 proj[12];` — reuse that machinery (frame codegen still defers it;
            // task #19). Extra brackets/stars/initializers are unmeasured.
            if let Some((element, total, _inner)) = self.last_array_typedef.take() {
                if is_static || total == 0 {
                    return Err(Diagnostic::error("a static or row-pointer array-typedef local is not supported yet (roadmap)"));
                }
                loop {
                    let name = self.parse_identifier()?;
                    if matches!(
                        self.peek(),
                        Token::BracketOpen | Token::Equals | Token::Star
                    ) {
                        return Err(Diagnostic::error("an array-typedef local with brackets/initializer is not supported yet (roadmap)"));
                    }
                    locals.push(LocalDeclaration {
                        declared_type: element,
                        name: name.clone(),
                        initializer: None,
                        array_length: Some(total),
                        is_static: false,
                        data_bytes: None,
                        data_relocations: Vec::new(),
                        is_const: false,
                        row_bytes: (_inner > 1).then(|| _inner * (element.width() as u16 / 8)),
                    });
                    self.variable_types.insert(name.clone(), element);
                    self.variable_array_bytes
                        .insert(name.clone(), element.width() as u32 / 8 * total as u32);
                    if !self.eat_keyword(Token::Comma) {
                        break;
                    }
                }
                self.expect(Token::Semicolon)?;
                continue;
            }
            let struct_tag = self.last_struct_tag.take();
            // One or more comma-separated declarators, each optionally initialized.
            loop {
                // `RET (*name)(params)` / `RET (**name)(params)` — a function-
                // pointer (or pointer to one) LOCAL: a 4-byte word; the signature
                // is skipped (abort_exit's `void (**var_r31)(void);`).
                if *self.peek() == Token::ParenOpen
                    && self.tokens.get(self.position + 1) == Some(&Token::Star)
                {
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
                            Token::EndOfFile => {
                                return Err(Diagnostic::error(
                                    "unterminated function-pointer local",
                                ))
                            }
                            _ => {}
                        }
                    }
                    let initializer = if self.eat_keyword(Token::Equals) {
                        Some(self.expression()?)
                    } else {
                        None
                    };
                    locals.push(LocalDeclaration {
                        declared_type: Type::Pointer(Pointee::Pointer),
                        name,
                        initializer,
                        array_length: None,
                        is_static: false,
                        data_bytes: None,
                        data_relocations: Vec::new(),
                        is_const: false,
                        row_bytes: None,
                    });
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
                        return Err(Diagnostic::error(
                            "a multi-level pointer declarator list is not supported yet (roadmap)",
                        ));
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
                let mut inner_elements: u16 = 1;
                let array_length = if *self.peek() == Token::BracketOpen {
                    self.advance();
                    let explicit = if *self.peek() == Token::BracketClose {
                        None
                    } else {
                        Some(self.parse_integer_constant()? as u16)
                    };
                    self.expect(Token::BracketClose)?;
                    // Further dimensions FLATTEN row-major into one frame slot of the
                    // product size — exactly the array-typedef local's representation
                    // (`Mtx m;` = `f32 m[12]`, proven byte-exact); `float m[3][4];` is
                    // the same object declared directly. Element access still defers in
                    // codegen; an initializer on a flattened array is unmeasured.
                    let mut explicit = explicit;
                    while *self.peek() == Token::BracketOpen {
                        self.advance();
                        let extra = self.parse_integer_constant()? as u16;
                        self.expect(Token::BracketClose)?;
                        inner_elements = inner_elements.saturating_mul(extra);
                        match explicit {
                            Some(length) => explicit = Some(length.saturating_mul(extra)),
                            None => return Err(Diagnostic::error("a multi-dimensional local array needs explicit dimensions (roadmap)")),
                        }
                    }
                    if *self.peek() == Token::Equals {
                        self.advance();
                        if is_static
                            && matches!(
                                declared_type,
                                Type::Pointer(_) | Type::StructPointer { .. }
                            )
                        {
                            // Function/data-pointer static arrays carry zero word images plus
                            // ADDR32 relocations, just like their file-scope counterparts.
                            let elements = self.parse_address_initializer()?;
                            let count = u16::try_from(elements.len()).map_err(|_| {
                                Diagnostic::error("too many static pointer initializer elements")
                            })?;
                            let length = explicit.unwrap_or(count);
                            let mut bytes = vec![0u8; usize::from(length) * 4];
                            for (index, element) in elements.into_iter().enumerate() {
                                let offset = index as u32 * 4;
                                match element {
                                    PointerElement::Null => {}
                                    PointerElement::Scalar(value) => bytes
                                        [index * 4..index * 4 + 4]
                                        .copy_from_slice(&(value as u32).to_be_bytes()),
                                    PointerElement::Symbol(target) => {
                                        data_relocations.push((offset, target, 0));
                                    }
                                    PointerElement::Str(_) => {
                                        return Err(Diagnostic::error("a string in a static-local pointer array needs function-local pooling (roadmap)"));
                                    }
                                }
                            }
                            data_bytes = Some(bytes);
                            Some(length)
                        } else if is_static
                            && matches!(declared_type, Type::Struct { .. })
                            && struct_tag.is_some()
                        {
                            // Preserve field widths/padding and address fields for a static
                            // array of structs. The same relocation-aware serializer serves
                            // file-scope struct arrays.
                            let tag = struct_tag.as_ref().unwrap();
                            let bytes =
                                self.parse_struct_array_initializer(tag, &mut data_relocations)?;
                            let element_size = match declared_type {
                                Type::Struct { size, .. } => size as usize,
                                _ => unreachable!(),
                            };
                            let count =
                                u16::try_from(bytes.len() / element_size).map_err(|_| {
                                    Diagnostic::error("too many static struct initializer elements")
                                })?;
                            data_bytes = Some(bytes);
                            Some(explicit.unwrap_or(count))
                        } else {
                            // An AUTOMATIC initialized array parses like the static
                            // form (its byte image on the local); native frame copy-in
                            // remains a generator concern.
                            if inner_elements > 1 && !is_static {
                                return Err(Diagnostic::error("an automatic multi-dimensional array initializer is not supported yet (roadmap)"));
                            }
                            if inner_elements > 1 {
                                let values = self.parse_constant_initializer(declared_type)?;
                                let count = u16::try_from(values.len()).map_err(|_| {
                                    Diagnostic::error("too many static array initializer elements")
                                })?;
                                let mut bytes = Vec::new();
                                for value in values {
                                    match declared_type {
                                        Type::Float | Type::Int | Type::UnsignedInt => bytes
                                            .extend_from_slice(&(value as u32).to_be_bytes()),
                                        Type::Double => bytes
                                            .extend_from_slice(&(value as u64).to_be_bytes()),
                                        Type::Char | Type::UnsignedChar => bytes.push(value as u8),
                                        Type::Short | Type::UnsignedShort => bytes
                                            .extend_from_slice(&(value as u16).to_be_bytes()),
                                        _ => return Err(Diagnostic::error("a multi-dimensional static-local initializer element is not supported yet (roadmap)")),
                                    }
                                }
                                data_bytes = Some(bytes);
                                Some(explicit.unwrap_or(count))
                            } else {
                            self.expect(Token::BraceOpen)?;
                            let mut bytes = Vec::new();
                            let mut count = 0u16;
                            loop {
                                if *self.peek() == Token::BraceClose {
                                    break;
                                }
                                // A float-literal element (optionally negated) keeps the direct
                                // read; any other element is a CONSTANT EXPRESSION — enums,
                                // shifts, arithmetic (`1 << 4`, `-A`) — parsed and folded.
                                let is_float = matches!(self.peek(), Token::FloatLiteral(_))
                                    || (*self.peek() == Token::Minus
                                        && matches!(self.peek_at(1), Token::FloatLiteral(_)));
                                if is_float {
                                    let negative = self.eat_keyword(Token::Minus);
                                    let Token::FloatLiteral(value) = self.advance().clone() else {
                                        unreachable!()
                                    };
                                    let value = if negative { -value } else { value };
                                    match declared_type {
                                        Type::Float => bytes.extend_from_slice(&(value as f32).to_be_bytes()),
                                        Type::Double => bytes.extend_from_slice(&value.to_be_bytes()),
                                        _ => return Err(Diagnostic::error("a float element in an integer static array is not supported yet (roadmap)")),
                                    }
                                } else {
                                    let value = self.parse_integer_constant()?;
                                    match declared_type {
                                        Type::Float => bytes.extend_from_slice(&(value as f32).to_be_bytes()),
                                        Type::Double => bytes.extend_from_slice(&(value as f64).to_be_bytes()),
                                        Type::Int | Type::UnsignedInt => bytes.extend_from_slice(&(value as i32).to_be_bytes()),
                                        Type::Char | Type::UnsignedChar => bytes.push(value as u8),
                                        Type::Short | Type::UnsignedShort => bytes.extend_from_slice(&(value as i16).to_be_bytes()),
                                        _ => return Err(Diagnostic::error("a static local array initializer element is not supported yet (roadmap)")),
                                    }
                                }
                                count += 1;
                                if !self.eat_keyword(Token::Comma) {
                                    break;
                                }
                            }
                            self.expect(Token::BraceClose)?;
                            data_bytes = Some(bytes);
                            Some(explicit.unwrap_or(count))
                            }
                        }
                    } else {
                        match explicit {
                            Some(length) => Some(length),
                            None => {
                                return Err(Diagnostic::error(
                                    "an array with no length needs an initializer",
                                ))
                            }
                        }
                    }
                } else {
                    None
                };
                let initializer = if array_length.is_none() && self.eat_keyword(Token::Equals) {
                    if *self.peek() == Token::BraceOpen {
                        // A static struct local is a data object, not a frame
                        // initialization. Serialize its complete layout and retain
                        // any address fields as object relocations.
                        if is_static
                            && matches!(declared_type, Type::Struct { .. })
                            && struct_tag.is_some()
                        {
                            let tag = struct_tag.as_ref().unwrap();
                            let image =
                                self.parse_one_struct_relocated(tag, 0, &mut data_relocations)?;
                            data_bytes = Some(image);
                            None
                        } else {
                            // A SMALL (<= 4 byte) STRUCT-typed local's brace initializer
                            // serializes to its byte image at parse time (the layout lives
                            // here, not in codegen) — `GXColor c = {0xFF,0xFF,0xFF,0xFF};`
                            // becomes a one-word image the frame path copies in from the
                            // pool. Larger structs keep the aggregate-literal parse (their
                            // committed handling flows through it); a relocated element
                            // (`{&g, 0}`) is not an image — defer.
                            let small_struct_tag = match (declared_type, struct_tag.as_ref()) {
                                (Type::Struct { size, .. }, Some(tag))
                                    if matches!(size, 4 | 8 | 12 | 16) =>
                                {
                                    Some(tag.clone())
                                }
                                _ => None,
                            };
                            if let Some(tag) = small_struct_tag {
                                let tag = &tag;
                                let mut relocations = Vec::new();
                                let image =
                                    self.parse_one_struct_relocated(tag, 0, &mut relocations)?;
                                if !relocations.is_empty() {
                                    return Err(Diagnostic::error("a relocated struct-local initializer is not supported yet (roadmap)"));
                                }
                                data_bytes = Some(image);
                                None
                            } else {
                                Some(self.aggregate_literal()?)
                            }
                        }
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
                    self.variable_array_bytes
                        .insert(name.clone(), element_bytes * length as u32);
                }
                locals.push(LocalDeclaration {
                    declared_type,
                    name,
                    initializer,
                    array_length,
                    is_static,
                    data_bytes,
                    data_relocations,
                    is_const: self.last_type_was_const,
                    row_bytes: (inner_elements > 1)
                        .then(|| inner_elements * (declared_type.width() as u16 / 8)),
                });
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
        let mut local_names: std::collections::HashSet<String> =
            locals.iter().map(|local| local.name.clone()).collect();
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
                        let statement =
                            self.parse_if_statement(&mut local_names, &mut block_locals)?;
                        statements.push(statement);
                        continue;
                    }
                    break;
                }
                if matches!(
                    self.peek(),
                    Token::KeywordWhile | Token::KeywordDo | Token::KeywordFor
                ) {
                    statements
                        .push(self.parse_loop_statement(&mut local_names, &mut block_locals)?);
                    continue;
                }
                if let Some(statement) = self.parse_jump_statement()? {
                    statements.push(statement);
                    continue;
                }
                // C++ and C99 allow a declaration after earlier statements in
                // the function's outermost block. Reuse the block declaration
                // parser: it hoists storage to the Function while preserving an
                // initializer as a positioned assignment.
                if self.peek_is_type()
                    || self.peek_is_local_array_typedef()
                    || matches!(self.peek(), Token::Identifier(word) if word == "static")
                {
                    self.parse_block_declaration(
                        &mut local_names,
                        &mut block_locals,
                        &mut statements,
                    )?;
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
                        || (*self.peek() == Token::BraceOpen
                            && *self.peek_at(1) == Token::KeywordReturn);
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
                        statements.extend(
                            self.parse_block_or_statement(&mut local_names, &mut block_locals)?,
                        );
                        continue 'body;
                    }
                    // `if (c) return v; else return d;` is the guard `if (c) return v;`
                    // with fall-through `d` — routed through the guard codegen (which
                    // normalizes a negated `!c` to keep `v` as the in-place default, as
                    // mwcc does) rather than emitted as a bare `(c) ? v : d` ternary.
                    let Some(otherwise) = self.parse_guard_return()? else {
                        return Err(Diagnostic::error(
                            "a bare `return;` in an else branch is not supported yet (roadmap)",
                        ));
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
        let mut terminal_return_line = None;
        let return_expression = if conditional_return.is_some() {
            conditional_return
        } else if *self.peek() == Token::KeywordReturn {
            terminal_return_line = Some(self.current_location().line);
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
        let body_end_line = self.current_location().line;
        self.expect(Token::BraceClose)?;
        for _ in 0..redundant_blocks {
            self.expect(Token::BraceClose)?;
        }

        for (alias, previous_type, was_function_pointer) in
            local_function_pointer_typedefs.into_iter().rev()
        {
            match previous_type {
                Some(previous) => {
                    self.typedefs.insert(alias.clone(), previous);
                }
                None => {
                    self.typedefs.remove(&alias);
                }
            }
            if was_function_pointer {
                self.function_pointer_typedefs.insert(alias);
            } else {
                self.function_pointer_typedefs.remove(&alias);
            }
        }

        let mut locals = locals;
        locals.extend(block_locals);
        self.function_sources
            .push(Some(mwcc_syntax_trees::FunctionSource {
                body_start_line,
                terminal_return_line,
                body_end_line,
            }));
        Ok(Function {
            return_type,
            name,
            is_static,
            is_weak: false,
            text_deferred: false,
            peephole_disabled: self.peephole_disabled,
            parameters,
            locals,
            statements,
            guards,
            return_expression,
            section: None,
            asm_body: None,
            force_active: self.force_active,
        })
    }

    /// Parse a function-scope `typedef RET (*Alias)(params);`. The alias registration is owned by
    /// `function_body`, which can restore a shadowed file-scope typedef at the closing brace.
    fn parse_local_function_pointer_typedef(&mut self) -> Compilation<String> {
        if !self.eat_word("typedef") {
            return Err(Diagnostic::error("expected a local typedef"));
        }
        self.parse_type()?;
        self.expect(Token::ParenOpen)?;
        self.expect(Token::Star)?;
        let alias = self.parse_identifier()?;
        self.expect(Token::ParenClose)?;
        self.expect(Token::ParenOpen)?;
        let mut depth = 1;
        while depth > 0 {
            match self.advance() {
                Token::ParenOpen => depth += 1,
                Token::ParenClose => depth -= 1,
                Token::EndOfFile => {
                    return Err(Diagnostic::error(
                        "unterminated local function-pointer typedef",
                    ))
                }
                _ => {}
            }
        }
        self.expect(Token::Semicolon)?;
        Ok(alias)
    }

    pub(crate) fn peek_is_type(&self) -> bool {
        if self.cplusplus
            && matches!(self.peek(), Token::Identifier(name) if self.struct_typedefs.contains_key(name))
            && *self.peek_at(1) == Token::Colon
            && *self.peek_at(2) == Token::Colon
        {
            return false;
        }
        self.token_starts_type(self.peek())
    }

    /// Whether the cursor sits on an array-typedef LOCAL declaration (`Mtx proj;`):
    /// an array-typedef name followed by a declarator identifier. Deliberately NOT
    /// part of `token_starts_type` — a `sizeof(Mtx)`/cast context would fold the
    /// DECAYED pointer size (4) instead of the array's (48); only the declaration
    /// sites, which handle the marker, may admit it.
    pub(crate) fn peek_is_local_array_typedef(&self) -> bool {
        matches!(self.peek(), Token::Identifier(name) if self.array_typedefs.contains_key(name))
            && matches!(self.peek_at(1), Token::Identifier(_))
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
                matches!(
                    word.as_str(),
                    "long" | "signed" | "double" | "const" | "volatile" | "register" | "enum"
                ) || self.typedefs.contains_key(word)
                    || self.struct_typedefs.contains_key(word)
                    || self.struct_pointer_typedefs.contains_key(word)
                    || (self.cplusplus
                        && (matches!(word.as_str(), "bool" | "wchar_t")
                            || self.enum_types.contains_key(word)))
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
        let Some(Statement::Return(Some(when_false))) = statements.pop() else {
            unreachable!()
        };
        let Some(Statement::If {
            condition,
            then_body,
            ..
        }) = statements.pop()
        else {
            unreachable!()
        };
        let Some(Statement::Return(Some(when_true))) = then_body.into_iter().next() else {
            unreachable!()
        };
        statements.push(Statement::Return(Some(Expression::Conditional {
            condition: Box::new(condition),
            when_true: Box::new(when_true),
            when_false: Box::new(when_false),
            origin: ConditionalOrigin::IfReturns,
        })));
    }
}

/// Lower a value-DISCARDED postfix step (`x++` as a statement or a
/// for-clause element) to its `x = x ± 1` desugar — exact when the value
/// is unused. Comma lists lower each element.
fn lower_discarded_post_step(expression: Expression) -> Expression {
    match expression {
        Expression::PostStep { target, operator } => {
            let value = Expression::Binary {
                operator,
                left: target.clone(),
                right: Box::new(Expression::IntegerLiteral(1)),
            };
            let value = indexed_update_value(&target, value);
            Expression::Assign {
                target,
                value: Box::new(value),
            }
        }
        Expression::Comma { left, right } => Expression::Comma {
            left: Box::new(lower_discarded_post_step(*left)),
            right: Box::new(lower_discarded_post_step(*right)),
        },
        other => other,
    }
}
