//! Inline-`asm` function parsing: the `asm` function signature + body, its
//! per-line label/entry/mnemonic/operand grammar, and the register-name lexer.
//! Part of the `items` module. Split from items/mod.rs (behavior-identical).

use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{AsmInstruction, AsmItem, AsmOperand, AsmRelocSuffix, Function, Type};
use mwcc_tokens::Token;
use crate::parser::Parser;

impl Parser {
    /// Parse a Metrowerks inline-`asm` function: the `asm` qualifier has been
    /// peeked (storage qualifiers already consumed). The C signature is scanned
    /// loosely — asm codegen names fixed registers, so only the function NAME and
    /// a `void` return matter; parameter types are consumed and discarded. Returns
    /// `None` for a bodyless prototype (`asm void f(void);`).
    pub(crate) fn parse_asm_function(&mut self, is_static: bool, is_weak: bool) -> Compilation<Option<Function>> {
        self.expect(Token::Asm)?;
        // The return type and name precede `(`; the last identifier is the name. A
        // `static`/`extern` qualifier may follow `asm` (mwcc allows `asm static void
        // f()`), so recognize it here too rather than only in the pre-`asm` loop.
        let mut return_type = Type::Void;
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
                Token::EndOfFile => return Err(Diagnostic::error("unterminated asm function signature")),
                other => {
                    // A non-`void` scalar return keeps the default `Void` type — it
                    // does not affect the emitted object for a bare asm function.
                    if *other == Token::KeywordInt || matches!(other, Token::KeywordChar | Token::KeywordShort | Token::KeywordUnsigned | Token::KeywordFloat) {
                        return_type = Type::Int;
                    }
                    self.advance();
                }
            }
        }
        // Consume the parameter list by paren-matching.
        self.expect(Token::ParenOpen)?;
        let mut depth = 1;
        while depth > 0 {
            match self.advance() {
                Token::ParenOpen => depth += 1,
                Token::ParenClose => depth -= 1,
                Token::EndOfFile => return Err(Diagnostic::error("unterminated asm parameter list")),
                _ => {}
            }
        }
        // A bodyless prototype ends here; there is nothing to define.
        if *self.peek() == Token::Semicolon {
            self.advance();
            return Ok(None);
        }
        self.expect(Token::BraceOpen)?;
        let asm_body = self.parse_asm_body()?;
        Ok(Some(Function {
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
                other => return Err(Diagnostic::error(format!("expected an asm mnemonic or label, found {other}"))),
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
                    other => return Err(Diagnostic::error(format!("expected a name after asm `entry`, found {other}"))),
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
                    Token::Newline | Token::Semicolon | Token::BraceClose | Token::EndOfFile => break,
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
                        Token::Identifier(s) if s == "ha" => ((value >> 16) + ((value >> 15) & 1)) & 0xffff,
                        Token::Identifier(s) if s == "l" => value & 0xffff,
                        other => return Err(Diagnostic::error(format!("unsupported asm numeric relocation suffix @{other}"))),
                    };
                    return Ok(AsmOperand::Immediate(part));
                }
                // A displacement memory operand: `<disp>(<gpr>)`.
                if *self.peek() == Token::ParenOpen {
                    self.advance();
                    let base = match self.advance() {
                        Token::Identifier(word) => match parse_asm_register(&word) {
                            Some(AsmOperand::Gpr(index)) => index,
                            _ => return Err(Diagnostic::error(format!("asm memory operand base '{word}' must be a general-purpose register"))),
                        },
                        other => return Err(Diagnostic::error(format!("expected a register in an asm memory operand, found {other}"))),
                    };
                    self.expect(Token::ParenClose)?;
                    let displacement = i16::try_from(value)
                        .map_err(|_| Diagnostic::error(format!("asm memory displacement {value} does not fit in 16 bits")))?;
                    return Ok(AsmOperand::Memory { displacement, base });
                }
                Ok(AsmOperand::Immediate(value))
            }
            // A register name; a `symbol@suffix` relocation reference; or (a bare
            // identifier) a branch-target label.
            Token::Identifier(word) => {
                if let Some(register) = parse_asm_register(&word) {
                    return Ok(register);
                }
                if *self.peek() == Token::At {
                    self.advance();
                    let suffix = match self.advance() {
                        Token::Identifier(s) if s == "h" => AsmRelocSuffix::Hi,
                        Token::Identifier(s) if s == "ha" => AsmRelocSuffix::Ha,
                        Token::Identifier(s) if s == "l" => AsmRelocSuffix::Lo,
                        other => return Err(Diagnostic::error(format!("unsupported asm relocation suffix @{other}"))),
                    };
                    return Ok(AsmOperand::Symbol { name: word, suffix });
                }
                Ok(AsmOperand::Label(word))
            }
            // A `@`-prefixed local label used as a branch target (`blt cr0, @exit`).
            Token::At => Ok(AsmOperand::Label(self.parse_asm_at_name()?)),
            other => Err(Diagnostic::error(format!("unexpected asm operand token {other}"))),
        }
    }

    /// Read the name after a `@` in an asm body: `@exit` (identifier) or `@1`
    /// (integer). Returns the name WITH its leading `@` so label defs and references
    /// use the same key.
    fn parse_asm_at_name(&mut self) -> Compilation<String> {
        match self.advance() {
            Token::Identifier(word) => Ok(format!("@{word}")),
            Token::IntegerLiteral(value) => Ok(format!("@{value}")),
            other => Err(Diagnostic::error(format!("expected a name after asm `@`, found {other}"))),
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
