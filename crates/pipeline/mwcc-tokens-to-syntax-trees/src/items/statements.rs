//! Statement parsing: blocks, if/loop/switch dispatch, jump/return/guard
//! statements, and the simple-statement classifier. Part of the `items` module.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{Expression, Function, GlobalDeclaration, GuardedReturn, LocalDeclaration, LoopKind, Parameter, Pointee, PointerElement, Statement, SwitchArm, TranslationUnit, Type};
use mwcc_tokens::Token;
use crate::parser::{Parser, StructField, StructLayout};
use super::*;

impl Parser {
    /// Parse one simple (non-control-flow) statement: a `switch`, an increment,
    /// an assignment / compound assignment / memory store, or a bare expression.
    /// Whether the `return` at the cursor is the function's TRAILING return:
    /// its statement-ending `;` is directly followed by the closing `}`. A
    /// return expression never contains a semicolon, so the first `;` ahead
    /// ends the statement.
    pub(crate) fn return_is_terminal(&self) -> bool {
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
    pub(crate) fn parse_jump_statement(&mut self) -> Compilation<Option<Statement>> {
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

    pub(crate) fn parse_simple_statement(&mut self, local_names: &mut std::collections::HashSet<String>, block_locals: &mut Vec<LocalDeclaration>) -> Compilation<Statement> {
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
    pub(crate) fn block_if_ahead(&self) -> bool {
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
    pub(crate) fn parse_guard_return(&mut self) -> Compilation<Option<Expression>> {
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
    pub(crate) fn parse_return_statement(&mut self) -> Compilation<Statement> {
        self.expect(Token::KeywordReturn)?;
        let value = if *self.peek() == Token::Semicolon { None } else { Some(self.expression()?) };
        self.expect(Token::Semicolon)?;
        Ok(Statement::Return(value))
    }

    /// A condition expression that may use the top-level comma operator
    /// (`if ((a = x), test)` — alloc.c's link/merge macros). Each left
    /// operand runs for side effects; the last operand is the value.
    pub(crate) fn parse_comma_expression(&mut self) -> Compilation<Expression> {
        let mut expression = self.expression()?;
        while *self.peek() == Token::Comma {
            self.advance();
            let right = self.expression()?;
            expression = Expression::Comma { left: Box::new(expression), right: Box::new(right) };
        }
        Ok(expression)
    }

    /// `if (condition) <block-or-statement> [else <block-or-statement> | else if]`.
    pub(crate) fn parse_if_statement(&mut self, local_names: &mut std::collections::HashSet<String>, block_locals: &mut Vec<LocalDeclaration>) -> Compilation<Statement> {
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
    pub(crate) fn parse_loop_statement(&mut self, local_names: &mut std::collections::HashSet<String>, block_locals: &mut Vec<LocalDeclaration>) -> Compilation<Statement> {
        match self.peek() {
            Token::KeywordWhile => {
                self.advance();
                self.expect(Token::ParenOpen)?;
                // The comma operator is legal in condition position —
                // `while (c = *s++, c != 0)` (strikers char_io puts).
                let mut condition = self.expression()?;
                while self.eat_keyword(Token::Comma) {
                    let right = self.expression()?;
                    condition = Expression::Comma { left: Box::new(condition), right: Box::new(right) };
                }
                let condition = Some(condition);
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
    pub(crate) fn comma_expression(&mut self) -> Compilation<Expression> {
        let mut expression = self.assignment_expression()?;
        while self.eat_keyword(Token::Comma) {
            let right = self.assignment_expression()?;
            expression = Expression::Comma { left: Box::new(expression), right: Box::new(right) };
        }
        Ok(expression)
    }

    /// A `{ ... }` block, or a single (non-`return`) statement, as a conditional
    /// branch body.
    pub(crate) fn parse_block_or_statement(&mut self, local_names: &mut std::collections::HashSet<String>, block_locals: &mut Vec<LocalDeclaration>) -> Compilation<Vec<Statement>> {
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
    pub(crate) fn parse_block(&mut self, local_names: &mut std::collections::HashSet<String>, block_locals: &mut Vec<LocalDeclaration>) -> Compilation<Vec<Statement>> {
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
            if self.peek_is_type() || self.peek_is_local_array_typedef() || matches!(self.peek(), Token::Identifier(word) if word == "static") {
                self.parse_block_declaration(local_names, block_locals, &mut statements)?;
                continue;
            }
            statements.push(self.parse_simple_statement(local_names, block_locals)?);
        }
        collapse_if_return_chain(&mut statements);
        self.expect(Token::BraceClose)?;
        self.block_renames.truncate(rename_depth);
        Ok(statements)
    }

    /// One declaration line inside a nested `{}` scope, a braced switch arm, or
    /// an if/loop body: the local hoists to the function (with a shadow rename
    /// when the name already exists) and any initializer becomes an `Assign` at
    /// its position. A `static` declaration parses like a function-level static
    /// local (bfbb ansi_fp's scoped `static double pow_10[8] = {…}` — a
    /// name$K-numbered data image).
    pub(crate) fn parse_block_declaration(&mut self, local_names: &mut std::collections::HashSet<String>, block_locals: &mut Vec<LocalDeclaration>, statements: &mut Vec<Statement>) -> Compilation<()> {
        let is_static = if matches!(self.peek(), Token::Identifier(word) if word == "static") {
            self.advance();
            true
        } else {
            false
        };
        {
                let declared_type = self.parse_type()?;
                if self.last_type_was_volatile {
                    return Err(Diagnostic::error("a volatile local is not supported yet (roadmap)"));
                }
                // An array-typedef local (`Mtx proj;`) is exactly the flat local array
                // `f32 proj[12];` — reuse that machinery (frame codegen still defers
                // it; task #19). Extra brackets/stars/initializers are unmeasured.
                let array_typedef_local = self.last_array_typedef.take();
                if let Some((element, total, _inner)) = array_typedef_local {
                    if is_static || total == 0 {
                        return Err(Diagnostic::error("a static or row-pointer array-typedef local is not supported yet (roadmap)"));
                    }
                    loop {
                        let name = self.parse_identifier()?;
                        let name = if local_names.contains(&name) {
                            self.rename_counter += 1;
                            let internal = format!("{name}@{}", self.rename_counter);
                            self.block_renames.push((name, internal.clone()));
                            internal
                        } else {
                            name
                        };
                        if matches!(self.peek(), Token::BracketOpen | Token::Equals | Token::Star) {
                            return Err(Diagnostic::error("an array-typedef local with brackets/initializer is not supported yet (roadmap)"));
                        }
                        block_locals.push(LocalDeclaration { declared_type: element, name: name.clone(), initializer: None, array_length: Some(total), is_static: false, data_bytes: None, is_const: false, row_bytes: (_inner > 1).then(|| _inner * (element.width() as u16 / 8)) });
                        local_names.insert(name.clone());
                        self.variable_types.insert(name.clone(), element);
                        self.variable_array_bytes.insert(name.clone(), element.width() as u32 / 8 * total as u32);
                        if !self.eat_keyword(Token::Comma) {
                            self.expect(Token::Semicolon)?;
                            return Ok(());
                        }
                    }
                }
                // A struct/union-typed local carries its tag so `cur->next` resolves
                // the layout — same as the function-top-level path. Nested-block
                // declarations (a `DestructorChain* cur` inside a while) went
                // unregistered before, so member access on them failed to type.
                let struct_tag = self.last_struct_tag.take();
                // parse_type consumes the FIRST declarator's `*` into the type;
                // a later declarator's own `*` MIRRORS it (`unsigned char *jp,
                // *kp;` — prime's ansi_fp), same as the function-level rule. A
                // mixed list (`int *p, q;`) defers rather than mis-typing q.
                let outer_is_pointer = matches!(declared_type, Type::Pointer(_) | Type::StructPointer { .. });
                let mut first_declarator = true;
                loop {
                    let mut declared_type = declared_type;
                    if self.eat_keyword(Token::Star) {
                        if *self.peek() == Token::Star {
                            return Err(Diagnostic::error("a pointer-to-pointer declarator in a nested block is not supported yet (roadmap)"));
                        }
                        if !outer_is_pointer {
                            declared_type = Type::Pointer(pointee_of(declared_type)?);
                        }
                    } else if outer_is_pointer && !first_declarator {
                        return Err(Diagnostic::error("a mixed pointer/non-pointer declarator list in a nested block is not supported yet (roadmap)"));
                    }
                    first_declarator = false;
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
                        if !is_static {
                            // `u8 text[36];` — an automatic block-scoped array
                            // hoists exactly like a function-level local array:
                            // a frame slot of N elements (strtold's digit buffer).
                            // A braced initializer here is still deferred.
                            self.advance();
                            let explicit = if *self.peek() == Token::BracketClose { None } else { Some(self.parse_integer_constant()? as u16) };
                            self.expect(Token::BracketClose)?;
                            // `char model[] = "INFINITY";` — the string image
                            // (with NUL) sizes the array; mwcc block-copies it
                            // into the frame slot (codegen defers un-captured).
                            let mut data_bytes = None;
                            if *self.peek() == Token::Equals {
                                self.advance();
                                match self.advance().clone() {
                                    Token::StringLiteral(bytes) => {
                                        let mut image = bytes.clone();
                                        image.push(0);
                                        data_bytes = Some(image);
                                    }
                                    _ => return Err(Diagnostic::error("a block-scoped array initializer is not supported yet (roadmap)")),
                                }
                            }
                            let length = match (explicit, &data_bytes) {
                                (Some(length), _) => length,
                                (None, Some(image)) => image.len() as u16,
                                (None, None) => return Err(Diagnostic::error("an unsized block-scoped array needs an initializer (roadmap)")),
                            };
                            block_locals.push(LocalDeclaration { declared_type, name: name.clone(), initializer: None, array_length: Some(length), is_static: false, data_bytes, is_const: false , row_bytes: None});
                            local_names.insert(name.clone());
                            self.variable_types.insert(name.clone(), declared_type);
                            let element_bytes = match declared_type {
                                Type::Struct { size, .. } => size as u32,
                                Type::Pointer(_) | Type::StructPointer { .. } => 4,
                                other => other.width() as u32 / 8,
                            };
                            self.variable_array_bytes.insert(name.clone(), element_bytes * length as u32);
                            if !self.eat_keyword(Token::Comma) {
                                self.expect(Token::Semicolon)?;
                                return Ok(());
                            }
                            continue;
                        }
                        // `static double pow_10[8] = { 1e1, … };` — parse the
                        // image exactly like a function-level static array.
                        self.advance();
                        let explicit = if *self.peek() == Token::BracketClose { None } else { Some(self.parse_integer_constant()? as u16) };
                        self.expect(Token::BracketClose)?;
                        self.expect(Token::Equals)?;
                        self.expect(Token::BraceOpen)?;
                        let mut bytes: Vec<u8> = Vec::new();
                        let mut count: u16 = 0;
                        loop {
                            let negative = self.eat_keyword(Token::Minus);
                            match (self.advance().clone(), declared_type) {
                                (Token::FloatLiteral(value), Type::Double) => {
                                    let value = if negative { -value } else { value };
                                    bytes.extend_from_slice(&value.to_be_bytes());
                                }
                                (Token::FloatLiteral(value), Type::Float) => {
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
                                _ => return Err(Diagnostic::error("a scoped static array initializer element is not supported yet (roadmap)")),
                            }
                            count += 1;
                            if !self.eat_keyword(Token::Comma) {
                                break;
                            }
                        }
                        self.expect(Token::BraceClose)?;
                        let length = explicit.unwrap_or(count);
                        let element_bytes = match declared_type {
                            Type::Struct { size, .. } => size as u32,
                            Type::Pointer(_) | Type::StructPointer { .. } => 4,
                            other => other.width() as u32 / 8,
                        };
                        self.variable_types.insert(name.clone(), declared_type);
                        self.variable_array_bytes.insert(name.clone(), element_bytes * length as u32);
                        block_locals.push(LocalDeclaration { declared_type, name: name.clone(), initializer: None, array_length: Some(length), is_static: true, data_bytes: Some(bytes), is_const: self.last_type_was_const , row_bytes: None});
                        local_names.insert(name);
                        if !self.eat_keyword(Token::Comma) {
                            self.expect(Token::Semicolon)?;
                            return Ok(());
                        }
                        continue;
                    }
                    block_locals.push(LocalDeclaration { declared_type, name: name.clone(), initializer: None, array_length: None, is_static, data_bytes: None, is_const: false , row_bytes: None});
                    local_names.insert(name.clone());
                    // Register the type so `sizeof(s_h)` (fdlibm's __HI/__LO
                    // macros inside e_pow's inner block) resolves at parse time.
                    self.variable_types.insert(name.clone(), declared_type);
                    if let Some(tag) = &struct_tag {
                        self.variable_structs.insert(name.clone(), tag.clone());
                    }
                    if *self.peek() == Token::Equals && is_static {
                        return Err(Diagnostic::error("a scalar static local initializer in a nested block is not supported yet (roadmap)"));
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
        }
        Ok(())
    }
}
