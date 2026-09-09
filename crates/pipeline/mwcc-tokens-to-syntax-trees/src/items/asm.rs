//! Inline-`asm` function parsing: the `asm` function signature + body, its
//! per-line label/entry/mnemonic/operand grammar, and the register-name lexer.
//! Part of the `items` module. Split from items/mod.rs (behavior-identical).

use crate::parser::Parser;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{
    AsmInstruction, AsmItem, AsmOperand, AsmRelocSuffix, Function, Parameter, Type,
};
use mwcc_tokens::Token;

impl Parser {
    /// Bind C `register` parameters to their incoming EABI general registers
    /// while parsing embedded asm blocks. Floating parameters use the separate
    /// FPR argument sequence and therefore do not consume a GPR; multiword and
    /// aggregate values advance the ABI cursor but are not representable as one
    /// named asm operand in the compact instruction model.
    pub(crate) fn configure_embedded_asm_parameters(
        &mut self,
        parameters: &[Parameter],
        register_parameters: &std::collections::HashSet<String>,
    ) {
        self.asm_parameters.clear();
        let mut next_general = 3u8;
        for parameter in parameters {
            let words = match parameter.parameter_type {
                Type::Float | Type::Double | Type::Void => 0,
                Type::LongLong | Type::UnsignedLongLong => 2,
                Type::Struct { size, .. } => {
                    u8::try_from(size.div_ceil(4).max(1)).unwrap_or(u8::MAX)
                }
                _ => 1,
            };
            if words == 1
                && next_general <= 10
                && register_parameters.contains(&parameter.name)
            {
                self.asm_parameters.push((
                    parameter.name.clone(),
                    next_general,
                    self.variable_structs.get(&parameter.name).cloned(),
                ));
            }
            next_general = next_general.saturating_add(words);
        }
    }

    /// Parse a Metrowerks inline-`asm` function. Storage qualifiers are already consumed;
    /// `asm_after_return_type` selects between `asm void f()` and `void asm f()`.
    /// Parameter declarations retain the ordinary source type metadata.
    /// Returns the parsed name alongside `None` for a bodyless prototype
    /// (`asm void f(void);`).  The caller still needs that name to retain
    /// declaration attributes and symbol-table ordering.
    pub(crate) fn parse_asm_function(
        &mut self,
        is_static: bool,
        is_weak: bool,
        asm_after_return_type: bool,
        prototypes: &mut Vec<(String, Type, Vec<Type>)>,
    ) -> Compilation<(String, Option<Function>)> {
        let mut is_static = is_static;
        let return_type = if asm_after_return_type {
            let return_type = self.parse_type()?;
            self.expect(Token::Asm)?;
            return_type
        } else {
            self.expect(Token::Asm)?;
            // Storage qualifiers may also follow `asm`. The return type must
            // use ordinary parsing: typedefs, pointer indirection and wide/FP
            // results determine the ABI at callers even for a verbatim body.
            while matches!(self.peek(), Token::Identifier(word) if word == "static" || word == "extern") {
                if matches!(self.peek(), Token::Identifier(word) if word == "static") { is_static = true; }
                self.advance();
            }
            self.parse_type()?
        };
        let name = self.parse_identifier()?;
        // Use the ordinary type parser: typedef identity and qualifiers matter
        // to debug information even though the body uses fixed registers.
        let source_parameters = self.parse_asm_parameters(&name)?;
        let register_parameters = source_parameters
            .iter()
            .map(|parameter| parameter.name.clone())
            .collect();
        self.configure_embedded_asm_parameters(&source_parameters, &register_parameters);
        for (parameter_name, _, tag) in &mut self.asm_parameters {
            *tag = self
                .function_parameter_structs
                .get(&(name.clone(), parameter_name.clone()))
                .cloned();
        }
        // A bodyless prototype ends here; there is nothing to define.
        if *self.peek() == Token::Semicolon {
            self.advance();
            self.asm_parameters.clear();
            prototypes.push((name.clone(), return_type,
                source_parameters.iter().map(|parameter| parameter.parameter_type).collect()));
            return Ok((name, None));
        }
        let body_start_line = self.current_location().line;
        self.expect(Token::BraceOpen)?;
        let asm_body = self.parse_asm_body();
        self.asm_parameters = Vec::new();
        let asm_body = asm_body?;
        let body_end_line = self.locations[self.position.saturating_sub(1)].line;
        self.function_sources
            .push(Some(mwcc_syntax_trees::FunctionSource {
                body_start_line,
                local_lines: Vec::new(),
                statement_lines: Vec::new(),
                leaf_statement_lines: Vec::new(),
                control_flow_lines: Vec::new(),
                terminal_return_line: None,
                body_end_line,
            }));
        Ok((
            name.clone(),
            Some(Function {
                text_deferred: false,
                peephole_disabled: self.peephole_disabled,
                return_type,
                name,
                is_static,
                is_weak,
                parameters: source_parameters,
                locals: Vec::new(),
                statements: Vec::new(),
                guards: Vec::new(),
                return_expression: None,
                section: None,
                preceded_by_asm: false,
                asm_body: Some(asm_body),
                inline_asm_blocks: Vec::new(),
                force_active: self.force_active,
            }),
        ))
    }

    fn parse_asm_parameters(&mut self, function_name: &str) -> Compilation<Vec<Parameter>> {
        self.expect(Token::ParenOpen)?;
        let mut parameters = Vec::new();
        while *self.peek() != Token::ParenClose {
            if *self.peek() == Token::Dot {
                self.advance();
                self.expect(Token::Dot)?;
                self.expect(Token::Dot)?;
                break;
            }
            let parameter_start = self.position;
            let mut parameter_type = self.parse_type()?;
            let source_type = parameter_type;
            let fundamental = self.last_source_fundamental.take();
            let pointee_const = self.last_type_was_const;
            let tag = self.last_struct_tag.take();
            let array_typedef = self.last_array_typedef.take();
            let array_typedef_row = self.last_array_typedef_row.take();
            if parameter_type == Type::Void
                && *self.peek() == Token::ParenClose
                && parameters.is_empty()
            {
                break;
            }
            let name = if let Token::Identifier(name) = self.peek().clone() {
                self.advance();
                name
            } else {
                String::new()
            };
            let (adjusted_type, extents) =
                self.parse_array_parameter_suffix(&name, parameter_type, array_typedef)?;
            parameter_type = adjusted_type;
            let row = Self::parameter_row_array(
                parameter_start,
                source_type,
                fundamental,
                array_typedef_row,
                &extents,
            )?;
            if !name.is_empty() {
                let key = (function_name.to_owned(), name.clone());
                if let Some(row) = row {
                    self.function_parameter_row_arrays.insert(key.clone(), row);
                }
                if let Some(tag) = tag {
                    self.function_parameter_structs.insert(key.clone(), tag);
                }
                if let Some(fundamental) = fundamental {
                    self.function_parameter_fundamentals
                        .insert(key.clone(), fundamental);
                }
                if pointee_const {
                    self.function_parameter_pointee_const.insert(key);
                }
            }
            parameters.push(Parameter {
                parameter_type,
                name,
            });
            if *self.peek() != Token::Comma {
                break;
            }
            self.advance();
        }
        self.expect(Token::ParenClose)?;
        Ok(parameters)
    }

    /// Parse the body items of an asm function up to the closing `}` (already past
    /// the opening `{`). asm is line-oriented: `Token::Newline` (emitted only inside
    /// asm blocks) separates instructions; blank lines are skipped. A leading
    /// `identifier :` is a label definition (`lbl_X:`), otherwise the line is a
    /// mnemonic and its operands.
    pub(crate) fn parse_asm_body(&mut self) -> Compilation<Vec<AsmItem>> {
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
            let source_line = self.current_location().line;
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
            // D-form loads/stores also accept a bare symbol or absolute
            // displacement. The symbol carries SDA21 with an unspecified base;
            // the linker chooses r2/r13 from the destination section.
            if matches!(
                mnemonic.as_str(),
                "lwz" | "lbz" | "lhz" | "lha" | "stw" | "stb" | "sth" | "lmw" | "stmw"
                    | "lfs" | "lfd" | "stfs" | "stfd"
            ) && operands.len() == 2
            {
                operands[1] = match &operands[1] {
                    AsmOperand::Label(name) => AsmOperand::SmallDataSymbolMemory {
                        name: name.clone(),
                        base: 0,
                    },
                    AsmOperand::Immediate(value) => AsmOperand::Memory {
                        displacement: i16::try_from(*value).map_err(|_| {
                            Diagnostic::error(format!(
                                "asm absolute displacement {value} does not fit in 16 bits"
                            ))
                        })?,
                        base: 0,
                    },
                    other => other.clone(),
                };
            }
            items.push(AsmItem::Instruction(AsmInstruction {
                mnemonic,
                operands,
                source_line,
            }));
        }
        Ok(items)
    }

    /// Parse one asm operand: a register, constant immediate, memory/member path,
    /// relocation, or label. Unknown forms error so the enclosing translation
    /// unit DEFERS rather than emitting wrong bytes.
    fn parse_asm_operand(&mut self) -> Compilation<AsmOperand> {
        let operand_start = self.position;
        if *self.peek() != Token::ParenOpen && super::asm_constants::starts_constant(self.peek()) {
            let value = self.parse_asm_constant(0)?;
            return self.finish_asm_integer_operand(i64::from(value));
        }
        match self.advance() {
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
                        let tag = tag.ok_or_else(|| {
                            Diagnostic::error(format!(
                                "asm parameter '{word}' has no struct type for member access"
                            ))
                        })?;
                        let offset = self.parse_asm_member_offset(tag)?;
                        return Ok(AsmOperand::Memory {
                            displacement: offset,
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
                    if *self.peek() == Token::ParenOpen {
                        let base = self.parse_asm_memory_base()?;
                        return Ok(AsmOperand::SymbolMemory {
                            name: word,
                            suffix,
                            base,
                        });
                    }
                    return Ok(AsmOperand::Symbol { name: word, suffix });
                }
                if *self.peek() == Token::ParenOpen {
                    let base = self.parse_asm_memory_base()?;
                    return Ok(AsmOperand::SmallDataSymbolMemory { name: word, base });
                }
                Ok(AsmOperand::Label(word))
            }
            // Parenthesized symbolic displacement arithmetic, used by the TRK runtime:
            // `(ProcessorState_PPC.Extended1.exceptionID + 2)(r2)`.
            Token::ParenOpen => {
                // The same surface syntax also wraps ordinary constant expressions in asm
                // immediates (`ori r3,r4,(1 << (31 - 16))`). Reparse the opening
                // parenthesis so following operators and memory suffixes are retained.
                if super::asm_constants::starts_constant(self.peek()) {
                    self.position = operand_start;
                    let value = self.parse_asm_constant(0)?;
                    return self.finish_asm_integer_operand(i64::from(value));
                }
                // Zero-displacement memory syntax: `(rN)` (or `(parameter)`).
                if *self.peek_at(1) == Token::ParenClose {
                    if let Token::Identifier(register) = self.peek().clone() {
                        let base = self.asm_memory_base_register(&register)?;
                        self.advance();
                        self.advance();
                        return Ok(AsmOperand::Memory {
                            displacement: 0,
                            base,
                        });
                    }
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
                        Token::IntegerLiteral(value)
                        | Token::UnsignedIntegerLiteral(value)
                        | Token::LongLongIntegerLiteral(value)
                        | Token::UnsignedLongLongIntegerLiteral(value) => value,
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

    /// Finish the suffix shared by literal and folded-unary integer operands.
    fn finish_asm_integer_operand(&mut self, value: i64) -> Compilation<AsmOperand> {
        // MWCC's assembler truncates the C constant to a signed word before
        // checking an instruction field, including explicitly wide literals.
        let value = i64::from(value as i32);
        // A `@`-suffix on a NUMERIC operand selects a 16-bit part of the value,
        // computed at assembly time (`lis r3, 0x7FFFFFFF@h`) — no relocation.
        if *self.peek() == Token::At {
            self.advance();
            let part = match self.advance() {
                Token::Identifier(s) if s == "h" => (value >> 16) & 0xffff,
                Token::Identifier(s) if s == "ha" => ((value >> 16) + ((value >> 15) & 1)) & 0xffff,
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
                    _ => match self
                        .asm_parameters
                        .iter()
                        .find(|(name, _, _)| *name == word)
                    {
                        Some(&(_, gpr, _)) => gpr,
                        None => {
                            return Err(Diagnostic::error(format!(
                            "asm memory operand base '{word}' must be a general-purpose register"
                        )))
                        }
                    },
                },
                other => {
                    return Err(Diagnostic::error(format!(
                        "expected a register in an asm memory operand, found {other}"
                    )))
                }
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

    /// Resolve `Tag.outer.words[3]` through the ordinary C layout table. The cursor starts on
    /// the first dot and stops after the final field/index, immediately before the `(rN)` base.
    fn parse_asm_struct_offset(&mut self, tag: String) -> Compilation<i16> {
        self.expect(Token::Dot)?;
        self.parse_asm_member_offset(tag)
    }

    /// Resolve a member path after its first separator. This is shared by
    /// `Tag.field[index](rN)` and register-parameter `value->field[index]`
    /// operands so both spellings use identical layout and bounds rules.
    fn parse_asm_member_offset(&mut self, mut tag: String) -> Compilation<i16> {
        let mut offset = 0i64;
        loop {
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
            self.advance();
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
        let base = self.asm_memory_base_register(&register)?;
        self.expect(Token::ParenClose)?;
        Ok(base)
    }

    fn asm_memory_base_register(&self, register: &str) -> Compilation<u8> {
        match parse_asm_register(register) {
            Some(AsmOperand::Gpr(index)) => Ok(index),
            _ => self
                .asm_parameters
                .iter()
                .find(|(name, _, _)| name == register)
                .map(|(_, gpr, _)| *gpr)
                .ok_or_else(|| {
                    Diagnostic::error(format!(
                        "asm memory operand base '{register}' must be a general-purpose register"
                    ))
                }),
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_syntax_trees::{AsmItem, AsmOperand};

    #[test]
    fn binds_embedded_asm_array_bases_to_register_parameters() {
        let source = r#"
            typedef float Mtx[3][4];
            void identity(register Mtx matrix) {
                asm {
                    psq_st f0, 8(matrix), 0, 0
                }
            }
        "#;
        let unit = crate::parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();
        assert!(matches!(
            unit.functions[0].inline_asm_blocks[0].items.as_slice(),
            [AsmItem::Instruction(instruction)]
                if instruction.operands[1]
                    == AsmOperand::Memory { displacement: 8, base: 3 }
        ));
    }

    #[test]
    fn floating_parameters_do_not_consume_embedded_asm_gprs() {
        let source = r#"
            void store(register float amount, register int* output) {
                asm {
                    stw r3, 0(output)
                }
            }
        "#;
        let unit = crate::parse_translation_unit(
            mwcc_source_to_tokens::tokenize(source).unwrap(),
            false,
            true,
            1,
            3,
        )
        .unwrap();
        assert!(matches!(
            unit.functions[0].inline_asm_blocks[0].items.as_slice(),
            [AsmItem::Instruction(instruction)]
                if instruction.operands[1]
                    == AsmOperand::Memory { displacement: 0, base: 3 }
        ));
    }
}
