//! Statement parsing: blocks, if/loop/switch dispatch, jump/return/guard
//! statements, and the simple-statement classifier. Part of the `items` module.

use super::*;
use crate::parser::{Parser, StructField, StructLayout};
use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{
    Expression, Function, GlobalDeclaration, GuardedReturn, LocalDeclaration, LoopKind, Parameter,
    Pointee, PointerElement, Statement, SwitchArm, TranslationUnit, Type,
};
use mwcc_tokens::Token;

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
            // A source label is one colon. `Class::member(...)` begins with the
            // same two tokens but must remain an expression statement.
            _ if *self.peek_at(1) == Token::Colon && *self.peek_at(2) != Token::Colon => {
                let name = self.parse_identifier()?;
                self.advance(); // the colon
                Ok(Some(Statement::Label(name)))
            }
            _ => Ok(None),
        }
    }

    pub(crate) fn parse_simple_statement(
        &mut self,
        local_names: &mut std::collections::HashSet<String>,
        block_locals: &mut Vec<LocalDeclaration>,
    ) -> Compilation<Statement> {
        if matches!(self.peek(), Token::Identifier(word) if word == "switch") {
            return self.parse_switch(local_names, block_locals);
        }
        if matches!(self.peek(), Token::Identifier(word) if word == "delete") {
            return self.parse_delete_statement();
        }
        let first = self.factor()?;
        // Preserve the nominal aggregate identity before parsing the right-hand
        // side. `expression_struct_tag` is a one-expression scratch slot, so the
        // RHS would otherwise overwrite the target's tag before a value copy can
        // be expanded into its typed scalar fields.
        let first_struct_tag = self.expression_struct_tag.take();
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
            let value = Expression::Binary {
                operator,
                left: Box::new(first.clone()),
                right: Box::new(rhs),
            };
            let value = super::indexed_update_value(&first, value);
            Ok(store_or_assign(first, value, local_names))
        } else if *self.peek() == Token::Equals {
            self.advance();
            let value = self.expression()?;
            let value_struct_tag = self.expression_struct_tag.take();
            self.expect(Token::Semicolon)?;
            if let Some(copy) = self.lower_typed_aggregate_assignment(
                &first,
                &value,
                first_struct_tag.as_deref(),
                value_struct_tag.as_deref(),
            ) {
                return Ok(Statement::Expression(copy));
            }
            Ok(store_or_assign(first, value, local_names))
        } else if *self.peek() == Token::Semicolon {
            self.advance();
            Ok(Statement::Expression(first))
        } else {
            // A discarded BINARY expression statement (`t & w;` — dead code in
            // MSL string.c): parse the full expression for a faithful AST; the
            // pure discarded form has no lowering yet, so codegen defers.
            let mut expression = self.binary_expression_from(first, 1)?;
            // The comma operator is also legal at the top level of a discarded
            // expression statement (`create(n), registerState(s);`). Call
            // argument parsing owns commas inside parentheses; any comma left
            // here sequences complete expressions from left to right.
            while self.eat_keyword(Token::Comma) {
                let right = self.assignment_expression()?;
                expression = Expression::Comma {
                    left: Box::new(expression),
                    right: Box::new(right),
                };
            }
            self.expect(Token::Semicolon)?;
            Ok(Statement::Expression(expression))
        }
    }

    /// Normalize scalar `delete pointer;` into the CodeWarrior EABI operation:
    /// null-check the object, then call its virtual deleting destructor with
    /// the compiler-supplied `1` destruction flag. Array delete and a direct
    /// non-virtual destructor remain explicit unsupported cases.
    fn parse_delete_statement(&mut self) -> Compilation<Statement> {
        self.advance(); // `delete`
        if *self.peek() == Token::BracketOpen {
            return Err(Diagnostic::error(
                "C++ array delete is not supported yet (roadmap)",
            ));
        }
        let object = self.expression()?;
        self.expect(Token::Semicolon)?;
        let class_name = match &object {
            Expression::Variable(name) => self.variable_structs.get(name).cloned(),
            _ => self.expression_struct_tag.take(),
        }
        .ok_or_else(|| Diagnostic::error("the class type of a delete target is not known"))?;
        let dispatch = self.resolve_virtual_deleting_destructor(&class_name)?;
        Ok(Statement::If {
            condition: Expression::Binary {
                operator: mwcc_syntax_trees::BinaryOperator::NotEqual,
                left: Box::new(object.clone()),
                right: Box::new(Expression::IntegerLiteral(0)),
            },
            then_body: vec![Statement::Expression(Expression::VirtualCall {
                object: Box::new(object),
                vptr_offset: dispatch.vptr_offset,
                slot_offset: dispatch.slot_offset,
                return_type: dispatch.return_type,
                variadic: dispatch.variadic,
                arguments: vec![Expression::IntegerLiteral(1)],
            })],
            else_body: Vec::new(),
        })
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
        let value = if *self.peek() == Token::Semicolon {
            None
        } else {
            Some(self.expression()?)
        };
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
            expression = Expression::Comma {
                left: Box::new(expression),
                right: Box::new(right),
            };
        }
        Ok(expression)
    }

    /// `if (condition) <block-or-statement> [else <block-or-statement> | else if]`.
    pub(crate) fn parse_if_statement(
        &mut self,
        local_names: &mut std::collections::HashSet<String>,
        block_locals: &mut Vec<LocalDeclaration>,
    ) -> Compilation<Statement> {
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
        Ok(Statement::If {
            condition,
            then_body,
            else_body,
        })
    }

    /// A `while`, `do … while`, or `for` loop. The body is a `{ … }` block or a
    /// single statement; the for-clause `init`/`step` are expressions (an `i = 0`
    /// assignment, an `i++` increment), any of which may be empty.
    pub(crate) fn parse_loop_statement(
        &mut self,
        local_names: &mut std::collections::HashSet<String>,
        block_locals: &mut Vec<LocalDeclaration>,
    ) -> Compilation<Statement> {
        match self.peek() {
            Token::KeywordWhile => {
                self.advance();
                self.expect(Token::ParenOpen)?;
                // The comma operator is legal in condition position —
                // `while (c = *s++, c != 0)` (strikers char_io puts).
                let mut condition = self.expression()?;
                while self.eat_keyword(Token::Comma) {
                    let right = self.expression()?;
                    condition = Expression::Comma {
                        left: Box::new(condition),
                        right: Box::new(right),
                    };
                }
                let condition = Some(condition);
                self.expect(Token::ParenClose)?;
                let body = self.parse_block_or_statement(local_names, block_locals)?;
                Ok(Statement::Loop {
                    kind: LoopKind::While,
                    initializer: None,
                    condition,
                    step: None,
                    body,
                })
            }
            Token::KeywordDo => {
                self.advance();
                let body = self.parse_block_or_statement(local_names, block_locals)?;
                self.expect(Token::KeywordWhile)?;
                self.expect(Token::ParenOpen)?;
                let condition = Some(self.expression()?);
                self.expect(Token::ParenClose)?;
                self.expect(Token::Semicolon)?;
                Ok(Statement::Loop {
                    kind: LoopKind::DoWhile,
                    initializer: None,
                    condition,
                    step: None,
                    body,
                })
            }
            Token::KeywordFor => {
                self.advance();
                self.expect(Token::ParenOpen)?;
                let rename_depth = self.block_renames.len();
                let initializer = if self.peek_is_type() {
                    let mut declaration_effects = Vec::new();
                    self.parse_block_declaration(
                        local_names,
                        block_locals,
                        &mut declaration_effects,
                    )?;
                    let mut effects = declaration_effects.into_iter().map(|statement| {
                        let Statement::Assign { name, value } = statement else {
                            return Err(Diagnostic::error(
                                "a for-init declaration produced an unsupported side effect",
                            ));
                        };
                        Ok(Expression::Assign {
                            target: Box::new(Expression::Variable(name)),
                            value: Box::new(value),
                        })
                    });
                    let mut initializer = effects.next().transpose()?;
                    for effect in effects {
                        initializer = Some(Expression::Comma {
                            left: Box::new(initializer.expect("a prior effect exists")),
                            right: Box::new(effect?),
                        });
                    }
                    initializer
                } else {
                    let initializer = (*self.peek() != Token::Semicolon)
                        .then(|| self.comma_expression())
                        .transpose()?
                        .map(lower_discarded_post_step);
                    self.expect(Token::Semicolon)?;
                    initializer
                };
                let condition = (*self.peek() != Token::Semicolon)
                    .then(|| self.expression())
                    .transpose()?;
                self.expect(Token::Semicolon)?;
                let step = (*self.peek() != Token::ParenClose)
                    .then(|| self.comma_expression())
                    .transpose()?
                    .map(lower_discarded_post_step);
                self.expect(Token::ParenClose)?;
                let body = self.parse_block_or_statement(local_names, block_locals)?;
                self.block_renames.truncate(rename_depth);
                Ok(Statement::Loop {
                    kind: LoopKind::For,
                    initializer,
                    condition,
                    step,
                    body,
                })
            }
            other => Err(Diagnostic::error(format!(
                "expected a loop keyword, found {other}"
            ))),
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
            expression = Expression::Comma {
                left: Box::new(expression),
                right: Box::new(right),
            };
        }
        Ok(expression)
    }

    /// A `{ ... }` block, or a single (non-`return`) statement, as a conditional
    /// branch body.
    pub(crate) fn parse_block_or_statement(
        &mut self,
        local_names: &mut std::collections::HashSet<String>,
        block_locals: &mut Vec<LocalDeclaration>,
    ) -> Compilation<Vec<Statement>> {
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
        if matches!(
            self.peek(),
            Token::KeywordWhile | Token::KeywordDo | Token::KeywordFor
        ) {
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
    pub(crate) fn parse_block(
        &mut self,
        local_names: &mut std::collections::HashSet<String>,
        block_locals: &mut Vec<LocalDeclaration>,
    ) -> Compilation<Vec<Statement>> {
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
            if matches!(
                self.peek(),
                Token::KeywordWhile | Token::KeywordDo | Token::KeywordFor
            ) {
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
            if (!self.peek_is_shadowed_member_base()
                && (self.peek_is_type() || self.peek_is_local_array_typedef()))
                || matches!(self.peek(), Token::Identifier(word) if word == "static")
            {
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
    pub(crate) fn parse_block_declaration(
        &mut self,
        local_names: &mut std::collections::HashSet<String>,
        block_locals: &mut Vec<LocalDeclaration>,
        statements: &mut Vec<Statement>,
    ) -> Compilation<()> {
        let mut is_static = false;
        let mut declaration_const = false;
        let mut declaration_volatile = false;
        while let Token::Identifier(word) = self.peek() {
            match word.as_str() {
                "static" => is_static = true,
                "const" => declaration_const = true,
                "volatile" => declaration_volatile = true,
                "register" | "auto" => {},
                _ => break,
            }
            self.advance();
        }
        {
            let declared_type = self.parse_type()?;
            self.last_type_was_const |= declaration_const;
            self.last_type_was_volatile |= declaration_volatile;
            let is_volatile = self.last_type_was_volatile;
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
                    if matches!(
                        self.peek(),
                        Token::BracketOpen | Token::Equals | Token::Star
                    ) {
                        return Err(Diagnostic::error("an array-typedef local with brackets/initializer is not supported yet (roadmap)"));
                    }
                    block_locals.push(LocalDeclaration {
                        declared_type: element,
                        name: name.clone(),
                        initializer: None,
                        is_volatile,
                        array_length: Some(total),
                        is_static: false,
                        data_bytes: None,
                        data_relocations: Vec::new(),
                        is_const: false,
                        row_bytes: (_inner > 1).then(|| _inner * (element.width() as u16 / 8)),
                    });
                    local_names.insert(name.clone());
                    self.variable_types.insert(name.clone(), element);
                    self.variable_array_bytes
                        .insert(name.clone(), element.width() as u32 / 8 * total as u32);
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
            // Aggregate references are represented as pointer values in the IR.
            // `parse_type` deliberately leaves `&` for ABI-aware consumers, so
            // a local declarator must consume it before reading the name.
            self.eat_keyword(Token::Ampersand);
            // parse_type consumes the FIRST declarator's `*` into the type;
            // a later declarator's own `*` MIRRORS it (`unsigned char *jp,
            // *kp;` — prime's ansi_fp), same as the function-level rule. A
            // mixed list (`int *p, q;`) defers rather than mis-typing q.
            let outer_is_pointer =
                matches!(declared_type, Type::Pointer(_) | Type::StructPointer { .. });
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
                        let explicit = if *self.peek() == Token::BracketClose {
                            None
                        } else {
                            Some(self.parse_integer_constant()? as u16)
                        };
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
                        let length =
                            match (explicit, &data_bytes) {
                                (Some(length), _) => length,
                                (None, Some(image)) => image.len() as u16,
                                (None, None) => return Err(Diagnostic::error(
                                    "an unsized block-scoped array needs an initializer (roadmap)",
                                )),
                            };
                        block_locals.push(LocalDeclaration {
                            declared_type,
                            name: name.clone(),
                            initializer: None,
                            is_volatile,
                            array_length: Some(length),
                            is_static: false,
                            data_bytes,
                            data_relocations: Vec::new(),
                            is_const: false,
                            row_bytes: None,
                        });
                        local_names.insert(name.clone());
                        self.variable_types.insert(name.clone(), declared_type);
                        let element_bytes = match declared_type {
                            Type::Struct { size, .. } => size as u32,
                            Type::Pointer(_) | Type::StructPointer { .. } => 4,
                            other => other.width() as u32 / 8,
                        };
                        self.variable_array_bytes
                            .insert(name.clone(), element_bytes * length as u32);
                        if !self.eat_keyword(Token::Comma) {
                            self.expect(Token::Semicolon)?;
                            return Ok(());
                        }
                        continue;
                    }
                    // `static double pow_10[8] = { 1e1, … };` — parse the
                    // image exactly like a function-level static array.
                    self.advance();
                    let explicit = if *self.peek() == Token::BracketClose {
                        None
                    } else {
                        Some(self.parse_integer_constant()? as u16)
                    };
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
                        // C permits a trailing comma in an initializer list. Once
                        // consumed, `}` ends the list rather than beginning an
                        // unsupported phantom element (rdp's command-code table).
                        if *self.peek() == Token::BraceClose {
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
                    self.variable_array_bytes
                        .insert(name.clone(), element_bytes * length as u32);
                    block_locals.push(LocalDeclaration {
                        declared_type,
                        name: name.clone(),
                        initializer: None,
                        is_volatile,
                        array_length: Some(length),
                        is_static: true,
                        data_bytes: Some(bytes),
                        data_relocations: Vec::new(),
                        is_const: self.last_type_was_const,
                        row_bytes: None,
                    });
                    local_names.insert(name);
                    if !self.eat_keyword(Token::Comma) {
                        self.expect(Token::Semicolon)?;
                        return Ok(());
                    }
                    continue;
                }
                local_names.insert(name.clone());
                // Register the type so `sizeof(s_h)` (fdlibm's __HI/__LO
                // macros inside e_pow's inner block) resolves at parse time.
                self.variable_types.insert(name.clone(), declared_type);
                if let Some(tag) = &struct_tag {
                    self.variable_structs.insert(name.clone(), tag.clone());
                }
                let explicit_constructor =
                    !is_static && struct_tag.is_some() && *self.peek() == Token::ParenOpen;
                let implicit_default_constructor = !is_static
                    && struct_tag.as_deref().is_some_and(|class_name| {
                        self.has_declared_default_constructor(class_name)
                    })
                    && !matches!(self.peek(), Token::Equals | Token::BraceOpen);
                let constructor_call = if explicit_constructor || implicit_default_constructor {
                    let mut arguments = Vec::new();
                    if explicit_constructor {
                        self.expect(Token::ParenOpen)?;
                        if *self.peek() != Token::ParenClose {
                            loop {
                                arguments.push(self.expression()?);
                                if !self.eat_keyword(Token::Comma) {
                                    break;
                                }
                            }
                        }
                        self.expect(Token::ParenClose)?;
                    }
                    let class_name = struct_tag.as_deref().expect("checked above");
                    let constructor =
                        self.resolve_placement_constructor(class_name, &arguments)?;
                    let arguments = self.lower_placement_constructor_arguments(
                        class_name,
                        &constructor,
                        arguments,
                    );
                    let mut call_arguments = vec![Expression::AddressOf {
                        operand: Box::new(Expression::Variable(name.clone())),
                    }];
                    call_arguments.extend(arguments);
                    Some(Statement::Expression(Expression::Call {
                        name: constructor,
                        arguments: call_arguments,
                    }))
                } else {
                    None
                };
                let mut data_bytes = None;
                let mut data_relocations = Vec::new();
                let initializer = if is_static && self.eat_keyword(Token::Equals) {
                    if *self.peek() == Token::BraceOpen
                        && matches!(declared_type, Type::Struct { .. })
                        && struct_tag.is_some()
                    {
                        data_bytes = Some(self.parse_one_struct_relocated(
                            struct_tag.as_ref().unwrap(),
                            0,
                            &mut data_relocations,
                        )?);
                    } else if matches!(
                        declared_type,
                        Type::Int
                            | Type::UnsignedInt
                            | Type::Char
                            | Type::UnsignedChar
                            | Type::Short
                            | Type::UnsignedShort
                            | Type::Float
                            | Type::Double
                            | Type::LongLong
                            | Type::UnsignedLongLong
                    ) {
                        let value = self.parse_scalar_constant(declared_type)? as u64;
                        let width = type_size(declared_type) as usize;
                        data_bytes = Some(value.to_be_bytes()[8 - width..].to_vec());
                    } else {
                        return Err(Diagnostic::error(
                            "this nested static-local initializer needs relocation-aware storage (roadmap)",
                        ));
                    }
                    None
                } else if self.eat_keyword(Token::Equals) {
                    // A declaration inside a nested block may use the same
                    // braced aggregate syntax as a function-scope local. Keep
                    // it at the declaration's executable position as an
                    // assignment (block locals are hoisted in the AST); later
                    // lowering can either claim the aggregate shape or defer
                    // without losing the rest of the function.
                    let value = if *self.peek() == Token::BraceOpen {
                        self.aggregate_literal()?
                    } else {
                        self.expression()?
                    };
                    statements.push(Statement::Assign {
                        name: name.clone(),
                        value,
                    });
                    None
                } else {
                    None
                };
                block_locals.push(LocalDeclaration {
                    declared_type,
                    name: name.clone(),
                    initializer,
                    is_volatile,
                    array_length: None,
                    is_static,
                    data_bytes,
                    data_relocations,
                    is_const: false,
                    row_bytes: None,
                });
                if let Some(call) = constructor_call {
                    statements.push(call);
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
