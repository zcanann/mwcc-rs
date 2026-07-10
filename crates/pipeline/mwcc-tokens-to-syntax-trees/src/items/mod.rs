//! Top-level item parsing: declarations, inline handling, and the translation unit.
//! Statement parsing lives in `statements`; global initializer and static-data
//! parsing in `initializers`; type and struct/union-body parsing in `types`.
//!
//! Split from the former single items.rs (fire 536); behavior-identical.

mod statements;
mod asm;
mod initializers;
mod types;


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
    pub(crate) fn expression_calls(expression: &Expression, names: &std::collections::HashSet<String>) -> bool {
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
    pub(crate) fn statement_calls(statement: &Statement, names: &std::collections::HashSet<String>) -> bool {
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
    pub(crate) fn parse_enum_body(&mut self) -> Compilation<()> {
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
    pub(crate) fn parse_enum_value(&mut self) -> Compilation<i64> {
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

    pub(crate) fn parse_enum_primary(&mut self) -> Compilation<i64> {
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
    pub(crate) fn parse_integer_constant(&mut self) -> Compilation<i64> {
        let expression = self.expression()?;
        crate::expressions::fold_constant_expression(&expression)
    }

    /// Parse `switch (scrutinee) { case <int>: return E; ... default: return E; }`.
    /// The subset requires every arm to be a single `return`; fall-through, blocks,
    /// and non-constant case labels are not supported yet.
    pub(crate) fn parse_switch(&mut self, local_names: &mut std::collections::HashSet<String>, block_locals: &mut Vec<LocalDeclaration>) -> Compilation<Statement> {
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
    pub(crate) fn parse_switch_arm_body(&mut self, local_names: &mut std::collections::HashSet<String>, block_locals: &mut Vec<LocalDeclaration>) -> Compilation<(mwcc_syntax_trees::ArmBody, bool)> {
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
    pub(crate) fn try_record_inline_body(&mut self) {
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
    pub(crate) fn skipped_function_name(&self) -> Option<String> {
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

    pub(crate) fn inline_asm_function_name(&self) -> Option<String> {
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
                Token::Asm => has_asm = true,
                Token::Identifier(word) if word == "__asm" => has_asm = true,
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
    pub(crate) fn parse_skipped_inline_statics(&self) -> Compilation<(String, bool, Vec<SkippedStaticLocal>)> {
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

    pub(crate) fn inline_function_has_static_local(&self) -> bool {
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
            while let Token::Pragma(directive) = self.peek() {
                match directive.as_str() {
                    "push" => self.cplusplus_stack.push(self.cplusplus),
                    "pop" => self.cplusplus = self.cplusplus_stack.pop().unwrap_or(false),
                    "cplusplus on" => self.cplusplus = true,
                    "cplusplus off" => self.cplusplus = false,
                    "defer_codegen on" => self.defer_codegen = true,
                    "defer_codegen off" => self.defer_codegen = false,
                    "force_active on" => self.force_active = true,
                    "force_active off" | "force_active reset" => self.force_active = false,
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
            // A Metrowerks inline-`asm` function DEFINITION: `[static] asm <ret>
            // name(params) { <instructions> }`. Its body is assembled verbatim (no C
            // codegen), so it is parsed by its own path. A bodyless `asm` prototype
            // yields no definition. An `inline` asm function is NOT handled here — it
            // is a skipped inline helper (recorded as a local-UND symbol by the
            // error-recovery path), never emitted. (The `static`/`__declspec(weak)`
            // qualifiers already ran.)
            if *self.peek() == Token::Asm && !is_inline {
                if let Some(function) = self.parse_asm_function(is_static, is_weak)? {
                    functions.push(function);
                }
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
                let pointer_object_const = self.last_pointer_const;
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
                            // `extern T name[];` — an UNSIZED extern array (Runtime's
                            // `extern __eti_init_info _eti_init_info[];`). Its size is
                            // unknowable here, and it only feeds the SDA21-vs-ADDR16
                            // total-size <= 8 choice — mwcc addresses an unknown-size
                            // array absolutely (lis/addi, measured), so register it with
                            // a huge sentinel length. No data is emitted for an extern.
                            None if is_extern => Some(u16::MAX),
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
                    // A pointer global is object-const only when the `const` TRAILS the
                    // star (`void* const`); a leading `const void*` is pointee-const and
                    // stays writable. A non-pointer keeps the plain leading-const rule.
                    let object_is_const = if matches!(return_type, Type::Pointer(_) | Type::StructPointer { .. }) {
                        pointer_object_const
                    } else {
                        is_const
                    };
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
                if let Some(section) = &declspec_section {
                    self.section_functions.insert(name.clone(), section.clone());
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
            // The section may sit on the definition (mp4) or on an earlier prototype
            // (pikmin's DECL_SECT on the memcpy proto) — prefer the definition's.
            let proto_section = self.section_functions.get(&name).cloned();
            if self.defer_codegen {
                self.deferred_function_names.push(name.clone());
            }
            let mut function = self.function_body(return_type, name, is_static, parameters)?;
            function.is_weak = function_is_weak;
            function.section = declspec_section.clone().or(proto_section);
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
    pub(crate) fn item_is_initialized_definition(&self) -> bool {
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
    pub(crate) fn item_is_uninitialized_definition(&self) -> bool {
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
    pub(crate) fn skipped_inline_label_bump(&self) -> Compilation<Option<usize>> {
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

    pub(crate) fn item_is_function_definition(&self) -> bool {
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
    pub(crate) fn capture_skipped_typedef(&mut self) {
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

    pub(crate) fn function_body(&mut self, return_type: Type, name: String, is_static: bool, parameters: Vec<Parameter>) -> Compilation<Function> {
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
        Ok(Function { return_type, name, is_static, is_weak: false, parameters, locals, statements, guards, return_expression, section: None, asm_body: None, force_active: self.force_active })
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

