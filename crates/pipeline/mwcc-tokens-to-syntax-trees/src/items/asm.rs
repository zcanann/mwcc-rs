//! Inline-`asm` function parsing: the `asm` function signature + body, its
//! per-line label/entry/mnemonic/operand grammar, and the register-name lexer.
//! Part of the `items` module. Split from items/mod.rs (behavior-identical).

use crate::parser::Parser;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{AsmInstruction, AsmItem, AsmOperand, AsmRelocSuffix, Function, Type};
use mwcc_tokens::Token;

impl Parser {
    /// Parse a Metrowerks inline-`asm` function. Storage qualifiers are already consumed;
    /// `asm_after_return_type` selects between `asm void f()` and `void asm f()`.
    /// The remainder of the C signature is scanned
    /// loosely — asm codegen names fixed registers, so only the function NAME and
    /// a `void` return matter; parameter types are consumed and discarded. Returns
    /// `None` for a bodyless prototype (`asm void f(void);`).
    pub(crate) fn parse_asm_function(
        &mut self,
        is_static: bool,
        is_weak: bool,
        asm_after_return_type: bool,
    ) -> Compilation<Option<Function>> {
        let mut return_type = if asm_after_return_type {
            let return_type = self.parse_type()?;
            self.expect(Token::Asm)?;
            return_type
        } else {
            self.expect(Token::Asm)?;
            Type::Void
        };
        // The return type and name precede `(`; the last identifier is the name. A
        // `static`/`extern` qualifier may follow `asm` (mwcc allows `asm static void
        // f()`), so recognize it here too rather than only in the pre-`asm` loop.
        let mut name = String::new();
        let mut is_static = is_static;
        loop {
            match self.peek() {
                Token::ParenOpen => break,
                Token::Identifier(word) if word == "static" => {
                    is_static = true;
                    self.advance();
                }
                Token::Identifier(word) if word == "extern" => {
                    self.advance();
                }
                Token::Identifier(word) => {
                    name = word.clone();
                    self.advance();
                }
                Token::EndOfFile => {
                    return Err(Diagnostic::error("unterminated asm function signature"))
                }
                other => {
                    // A non-`void` scalar return keeps the default `Void` type — it
                    // does not affect the emitted object for a bare asm function.
                    if *other == Token::KeywordInt
                        || matches!(
                            other,
                            Token::KeywordChar
                                | Token::KeywordShort
                                | Token::KeywordUnsigned
                                | Token::KeywordFloat
                        )
                    {
                        return_type = Type::Int;
                    }
                    self.advance();
                }
            }
        }
        // Consume the parameter list, capturing REGISTER PARAMETER names so the body's
        // operands can name them (`mr r3,val`; `stw r5,env->pc`). Integer/pointer
        // parameters take the positional argument registers r3, r4, …; the LAST
        // identifier of each comma-separated parameter that is not a qualifier is its
        // name, and an identifier naming a declared struct is the parameter's tag (for
        // member-operand offsets). A float/double parameter would take an FPR — not
        // needed by any measured asm function, so it defers.
        self.expect(Token::ParenOpen)?;
        let mut parameters: Vec<(String, u8, Option<String>)> = Vec::new();
        let mut parameter_name: Option<String> = None;
        let mut parameter_tag: Option<String> = None;
        let mut parameter_is_float = false;
        let mut depth = 1;
        loop {
            let token = self.advance();
            let end_of_parameter = matches!(token, Token::Comma) && depth == 1;
            match token {
                Token::ParenOpen => depth += 1,
                Token::ParenClose => {
                    depth -= 1;
                    if depth == 0 {
                        if let Some(name) = parameter_name.take() {
                            if !parameter_is_float {
                                parameters.push((
                                    name,
                                    3 + parameters.len() as u8,
                                    parameter_tag.take(),
                                ));
                            }
                        }
                        break;
                    }
                }
                // A float/double parameter lives in an FPR (f1, …) and — per the EABI —
                // consumes NO integer argument register, so it is skipped: the body
                // addresses it as `fp1` directly (measured: __cvt_fp2unsigned's
                // `register double d` is only ever fp1), never by name.
                Token::KeywordFloat => parameter_is_float = true,
                Token::Identifier(word) if word == "double" => parameter_is_float = true,
                Token::Identifier(word) if word != "register" && word != "const" => {
                    if self.structs.contains_key(word.as_str()) {
                        parameter_tag = Some(word.clone());
                    }
                    parameter_name = Some(word.clone());
                }
                Token::EndOfFile => {
                    return Err(Diagnostic::error("unterminated asm parameter list"))
                }
                _ => {}
            }
            if end_of_parameter {
                if let Some(name) = parameter_name.take() {
                    if !parameter_is_float {
                        parameters.push((name, 3 + parameters.len() as u8, parameter_tag.take()));
                    }
                }
                parameter_is_float = false;
            }
        }
        // A bodyless prototype ends here; there is nothing to define.
        if *self.peek() == Token::Semicolon {
            self.advance();
            return Ok(None);
        }
        let body_start_line = self.current_location().line;
        self.expect(Token::BraceOpen)?;
        self.asm_parameters = parameters;
        let asm_body = self.parse_asm_body();
        self.asm_parameters = Vec::new();
        let asm_body = asm_body?;
        let body_end_line = self.locations[self.position.saturating_sub(1)].line;
        self.function_sources
            .push(Some(mwcc_syntax_trees::FunctionSource {
                body_start_line,
                terminal_return_line: None,
                body_end_line,
            }));
        Ok(Some(Function {
            text_deferred: false,
            peephole_disabled: self.peephole_disabled,
            return_type,
            name,
            is_static,
            is_weak,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: None,
            section: None,
            asm_body: Some(asm_body),
            force_active: self.force_active,
        }))
    }

    /// Parse the body items of an asm function up to the closing `}` (already past
    /// the opening `{`). asm is line-oriented: `Token::Newline` (emitted only inside
    /// asm blocks) separates instructions; blank lines are skipped. A leading
    /// `identifier :` is a label definition (`lbl_X:`), otherwise the line is a
    /// mnemonic and its operands.
    fn parse_asm_body(&mut self) -> Compilation<Vec<AsmItem>> {
        let mut items = Vec::new();
        loop {
            // Blank separators between instructions: newlines, and (some sources —
            // BfBB's runtime.c) semicolons terminating each asm line.
            while matches!(self.peek(), Token::Newline | Token::Semicolon) {
                self.advance();
            }
            match self.peek() {
                Token::BraceClose => {
                    self.advance();
                    break;
                }
                Token::EndOfFile => return Err(Diagnostic::error("unterminated asm body")),
                // A `@`-prefixed local label definition (`@exit:`, `@1`). The colon is
                // optional (mwcc allows a bare `@1` before an instruction on the same
                // line), so this does not `continue` past a following instruction.
                Token::At => {
                    self.advance();
                    let name = self.parse_asm_at_name()?;
                    if *self.peek() == Token::Colon {
                        self.advance();
                    }
                    items.push(AsmItem::Label(name));
                    continue;
                }
                _ => {}
            }
            let mut mnemonic = match self.advance() {
                Token::Identifier(word) => word,
                other => {
                    return Err(Diagnostic::error(format!(
                        "expected an asm mnemonic or label, found {other}"
                    )))
                }
            };
            // `identifier :` is a label definition, not an instruction.
            if *self.peek() == Token::Colon {
                self.advance();
                items.push(AsmItem::Label(mnemonic));
                continue;
            }
            // `entry <name>` defines an additional global symbol at this position.
            if mnemonic == "entry" {
                let name = match self.advance() {
                    Token::Identifier(word) => word,
                    other => {
                        return Err(Diagnostic::error(format!(
                            "expected a name after asm `entry`, found {other}"
                        )))
                    }
                };
                items.push(AsmItem::Entry(name));
                continue;
            }
            // A `.` immediately after the mnemonic is the record-bit suffix
            // (`addic.`, `rlwinm.`, `or.`): the lexer split it off as its own token.
            if *self.peek() == Token::Dot {
                self.advance();
                mnemonic.push('.');
            }
            // A `+`/`-` immediately after a branch mnemonic is the static-prediction
            // hint (`ble+`): re-attach it so the assembler sets the BO hint bit.
            match self.peek() {
                Token::Plus => {
                    self.advance();
                    mnemonic.push('+');
                }
                Token::Minus => {
                    self.advance();
                    mnemonic.push('-');
                }
                _ => {}
            }
            let mut operands = Vec::new();
            loop {
                match self.peek() {
                    Token::Newline | Token::Semicolon | Token::BraceClose | Token::EndOfFile => {
                        break
                    }
                    Token::Comma => {
                        self.advance();
                    }
                    _ => operands.push(self.parse_asm_operand()?),
                }
            }
            items.push(AsmItem::Instruction(AsmInstruction { mnemonic, operands }));
        }
        Ok(items)
    }

    /// Parse one asm operand: a register name, or an (optionally negative) integer
    /// immediate. Unsupported operand forms (member `env->field`, labels) error, so
    /// the enclosing translation unit DEFERS rather than emitting wrong bytes.
    fn parse_asm_operand(&mut self) -> Compilation<AsmOperand> {
        let negate = *self.peek() == Token::Minus;
        if negate {
            self.advance();
        }
        match self.advance() {
            Token::IntegerLiteral(value) => {
                let value = if negate { -value } else { value };
                // A `@`-suffix on a NUMERIC operand selects a 16-bit part of the value,
                // computed at assembly time (`lis r3, 0x7FFFFFFF@h`) — no relocation.
                if *self.peek() == Token::At {
                    self.advance();
                    let part = match self.advance() {
                        Token::Identifier(s) if s == "h" => (value >> 16) & 0xffff,
                        Token::Identifier(s) if s == "ha" => {
                            ((value >> 16) + ((value >> 15) & 1)) & 0xffff
                        }
                        Token::Identifier(s) if s == "l" => value & 0xffff,
                        other => {
                            return Err(Diagnostic::error(format!(
                                "unsupported asm numeric relocation suffix @{other}"
                            )))
                        }
                    };
                    return Ok(AsmOperand::Immediate(part));
                }
                // A displacement memory operand: `<disp>(<gpr>)`.
                if *self.peek() == Token::ParenOpen {
                    self.advance();
                    let base = match self.advance() {
                        Token::Identifier(word) => match parse_asm_register(&word) {
                            Some(AsmOperand::Gpr(index)) => index,
                            // A named register PARAMETER as the base (`PTMF.f(ptmf)`).
                            _ => match self.asm_parameters.iter().find(|(name, _, _)| *name == word) {
                                Some(&(_, gpr, _)) => gpr,
                                None => return Err(Diagnostic::error(format!("asm memory operand base '{word}' must be a general-purpose register"))),
                            },
                        },
                        other => return Err(Diagnostic::error(format!("expected a register in an asm memory operand, found {other}"))),
                    };
                    self.expect(Token::ParenClose)?;
                    let displacement = i16::try_from(value).map_err(|_| {
                        Diagnostic::error(format!(
                            "asm memory displacement {value} does not fit in 16 bits"
                        ))
                    })?;
                    return Ok(AsmOperand::Memory { displacement, base });
                }
                Ok(AsmOperand::Immediate(value))
            }
            // A register name; a register PARAMETER (`mr r3,val`) or its member
            // (`stw r5,env->pc` — a displacement off the parameter's register); a
            // `symbol@suffix` relocation reference; or (a bare identifier) a
            // branch-target label.
            Token::Identifier(word) => {
                if let Some(register) = parse_asm_register(&word) {
                    return Ok(register);
                }
                // `Tag.field(rN)` — a struct-TAG-qualified field offset as a
                // displacement memory operand (`lwz r5, PTMF.this_delta(r3)` ->
                // `lwz r5, 0(r3)`, the field's offset off the base register).
                let struct_tag = self
                    .struct_typedefs
                    .get(&word)
                    .cloned()
                    .unwrap_or_else(|| word.clone());
                if self.structs.contains_key(&struct_tag) && *self.peek() == Token::Dot {
                    let displacement = self.parse_asm_struct_offset(struct_tag)?;
                    let base = self.parse_asm_memory_base()?;
                    return Ok(AsmOperand::Memory { displacement, base });
                }
                if let Some((_, gpr, tag)) = self
                    .asm_parameters
                    .iter()
                    .find(|(name, _, _)| *name == word)
                    .cloned()
                {
                    if *self.peek() == Token::Arrow {
                        self.advance();
                        let field = match self.advance() {
                            Token::Identifier(field) => field,
                            other => {
                                return Err(Diagnostic::error(format!(
                                    "expected a field name after '{word}->', found {other}"
                                )))
                            }
                        };
                        let tag = tag.ok_or_else(|| {
                            Diagnostic::error(format!(
                                "asm parameter '{word}' has no struct type for '->{field}'"
                            ))
                        })?;
                        let offset = self
                            .structs
                            .get(&tag)
                            .and_then(|layout| layout.fields.get(&field))
                            .map(|member| member.offset)
                            .ok_or_else(|| {
                                Diagnostic::error(format!("no field '{field}' in struct '{tag}'"))
                            })?;
                        return Ok(AsmOperand::Memory {
                            displacement: offset as i16,
                            base: gpr,
                        });
                    }
                    return Ok(AsmOperand::Gpr(gpr));
                }
                if *self.peek() == Token::At {
                    self.advance();
                    let suffix = match self.advance() {
                        Token::Identifier(s) if s == "h" => AsmRelocSuffix::Hi,
                        Token::Identifier(s) if s == "ha" => AsmRelocSuffix::Ha,
                        Token::Identifier(s) if s == "l" => AsmRelocSuffix::Lo,
                        other => {
                            return Err(Diagnostic::error(format!(
                                "unsupported asm relocation suffix @{other}"
                            )))
                        }
                    };
                    return Ok(AsmOperand::Symbol { name: word, suffix });
                }
                Ok(AsmOperand::Label(word))
            }
            // Parenthesized symbolic displacement arithmetic, used by the TRK runtime:
            // `(ProcessorState_PPC.Extended1.exceptionID + 2)(r2)`.
            Token::ParenOpen => {
                // The same surface syntax also wraps ordinary constant expressions in asm
                // immediates (`ori r3,r4,(1 << (31 - 16))`). Reuse the C constant folder; the
                // closing parenthesis naturally terminates its expression grammar.
                if matches!(self.peek(), Token::IntegerLiteral(_) | Token::Minus) {
                    let value = self.parse_integer_constant()?;
                    self.expect(Token::ParenClose)?;
                    return Ok(AsmOperand::Immediate(value));
                }
                let root = match self.advance() {
                    Token::Identifier(root) => root,
                    other => {
                        return Err(Diagnostic::error(format!(
                            "expected a struct name in an asm displacement expression, found {other}"
                        )))
                    }
                };
                let struct_tag = self
                    .struct_typedefs
                    .get(&root)
                    .cloned()
                    .unwrap_or(root.clone());
                if !self.structs.contains_key(&struct_tag) || *self.peek() != Token::Dot {
                    return Err(Diagnostic::error(format!(
                        "asm displacement expression '{root}' is not a declared struct path"
                    )));
                }
                let mut displacement = i64::from(self.parse_asm_struct_offset(struct_tag)?);
                if *self.peek() == Token::Plus || *self.peek() == Token::Minus {
                    let subtract = *self.peek() == Token::Minus;
                    self.advance();
                    let addend = match self.advance() {
                        Token::IntegerLiteral(value) => value,
                        other => {
                            return Err(Diagnostic::error(format!(
                                "expected an integer asm displacement addend, found {other}"
                            )))
                        }
                    };
                    displacement += if subtract { -addend } else { addend };
                }
                self.expect(Token::ParenClose)?;
                let displacement = i16::try_from(displacement).map_err(|_| {
                    Diagnostic::error(format!(
                        "asm symbolic displacement {displacement} does not fit in 16 bits"
                    ))
                })?;
                let base = self.parse_asm_memory_base()?;
                Ok(AsmOperand::Memory { displacement, base })
            }
            // A `@`-prefixed local label used as a branch target (`blt cr0, @exit`).
            Token::At => Ok(AsmOperand::Label(self.parse_asm_at_name()?)),
            other => Err(Diagnostic::error(format!(
                "unexpected asm operand token {other}"
            ))),
        }
    }

    /// Resolve `Tag.outer.words[3]` through the ordinary C layout table. The cursor starts on
    /// the first dot and stops after the final field/index, immediately before the `(rN)` base.
    fn parse_asm_struct_offset(&mut self, mut tag: String) -> Compilation<i16> {
        let mut offset = 0i64;
        loop {
            self.expect(Token::Dot)?;
            let field_name = match self.advance() {
                Token::Identifier(field) => field,
                other => {
                    return Err(Diagnostic::error(format!(
                        "expected a field name in asm struct path, found {other}"
                    )))
                }
            };
            let (field_offset, next_tag, array_element) = self
                .structs
                .get(&tag)
                .and_then(|layout| layout.fields.get(&field_name))
                .map(|field| (field.offset, field.struct_tag.clone(), field.array_element))
                .ok_or_else(|| {
                    Diagnostic::error(format!("no field '{field_name}' in struct '{tag}'"))
                })?;
            offset += i64::from(field_offset);

            if *self.peek() == Token::BracketOpen {
                self.advance();
                let index = self.parse_integer_constant()?;
                self.expect(Token::BracketClose)?;
                let element = array_element.ok_or_else(|| {
                    Diagnostic::error(format!(
                        "asm struct field '{tag}.{field_name}' is not an indexable scalar array"
                    ))
                })?;
                offset += index * i64::from(element.size());
            }

            if *self.peek() != Token::Dot {
                break;
            }
            tag = next_tag.ok_or_else(|| {
                Diagnostic::error(format!(
                    "asm struct field '{tag}.{field_name}' has no nested layout"
                ))
            })?;
        }
        i16::try_from(offset).map_err(|_| {
            Diagnostic::error(format!(
                "asm symbolic displacement {offset} does not fit in 16 bits"
            ))
        })
    }

    /// Parse the `(rN)` suffix shared by numeric and layout-derived memory operands.
    fn parse_asm_memory_base(&mut self) -> Compilation<u8> {
        self.expect(Token::ParenOpen)?;
        let register = match self.advance() {
            Token::Identifier(register) => register,
            other => {
                return Err(Diagnostic::error(format!(
                    "expected a register in an asm memory operand, found {other}"
                )))
            }
        };
        let base = match parse_asm_register(&register) {
            Some(AsmOperand::Gpr(index)) => index,
            _ => self
                .asm_parameters
                .iter()
                .find(|(name, _, _)| *name == register)
                .map(|(_, gpr, _)| *gpr)
                .ok_or_else(|| {
                    Diagnostic::error(format!(
                        "asm memory operand base '{register}' must be a general-purpose register"
                    ))
                })?,
        };
        self.expect(Token::ParenClose)?;
        Ok(base)
    }

    /// Read the name after a `@` in an asm body: `@exit` (identifier) or `@1`
    /// (integer). Returns the name WITH its leading `@` so label defs and references
    /// use the same key.
    fn parse_asm_at_name(&mut self) -> Compilation<String> {
        match self.advance() {
            Token::Identifier(word) => Ok(format!("@{word}")),
            Token::IntegerLiteral(value) => Ok(format!("@{value}")),
            other => Err(Diagnostic::error(format!(
                "expected a name after asm `@`, found {other}"
            ))),
        }
    }
}

/// Parse an inline-`asm` register operand name into an `AsmOperand`: `rN` (GPR),
/// `fpN`/`fN` (FPR) for 0..=31, or an alias (`sp`/`SP` → r1, `RTOC`/`rtoc` → r2).
/// Returns `None` for anything else (a label, a symbol, an unknown name).
fn parse_asm_register(word: &str) -> Option<AsmOperand> {
    match word {
        "sp" | "SP" => return Some(AsmOperand::Gpr(1)),
        "RTOC" | "rtoc" => return Some(AsmOperand::Gpr(2)),
        _ => {}
    }
    // A condition-register field `crN` (0..=7).
    if let Some(digits) = word.strip_prefix("cr") {
        if let Ok(field) = digits.parse::<u8>() {
            return (field <= 7).then_some(AsmOperand::ConditionRegister(field));
        }
    }
    let index = |digits: &str| -> Option<u8> {
        let value: u16 = digits.parse().ok()?;
        (value <= 31).then_some(value as u8)
    };
    // `fp` must be tried before the bare `f`/`r` prefixes (`fp14` also starts `f`).
    if let Some(digits) = word.strip_prefix("fp") {
        return index(digits).map(AsmOperand::Fpr);
    }
    if let Some(digits) = word.strip_prefix('r') {
        return index(digits).map(AsmOperand::Gpr);
    }
    if let Some(digits) = word.strip_prefix('f') {
        return index(digits).map(AsmOperand::Fpr);
    }
    None
}
