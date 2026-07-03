//! Precedence-climbing expression parsing: ternary, binary operators, prefix
//! unary operators, casts, and primary factors.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{BinaryOperator, Expression, Type, UnaryOperator};
use mwcc_tokens::Token;

use crate::parser::Parser;

/// Evaluate a *constant integer expression* (a global initializer element) to its
/// value. Enum constants are already folded to literals by the parser, so this
/// handles literals, the arithmetic/bitwise/shift/comparison/logical operators,
/// unary `-`/`~`/`!`, integer casts, and `?:`. Anything that is not an integer
/// constant — a symbol reference, an address-of, a float, a call — defers, so an
/// initializer needing a data relocation is never silently mis-encoded.
pub(crate) fn fold_constant_expression(expression: &Expression) -> Compilation<i64> {
    use BinaryOperator::*;
    Ok(match expression {
        Expression::IntegerLiteral(value) => *value,
        Expression::Unary { operator, operand } => {
            let value = fold_constant_expression(operand)?;
            match operator {
                UnaryOperator::Negate => value.wrapping_neg(),
                UnaryOperator::BitNot => !value,
                UnaryOperator::LogicalNot => (value == 0) as i64,
            }
        }
        Expression::Binary { operator, left, right } => {
            let left = fold_constant_expression(left)?;
            let right = fold_constant_expression(right)?;
            match operator {
                Add => left.wrapping_add(right),
                Subtract => left.wrapping_sub(right),
                Multiply => left.wrapping_mul(right),
                Divide if right == 0 => return Err(Diagnostic::error("constant division by zero")),
                Modulo if right == 0 => return Err(Diagnostic::error("constant modulo by zero")),
                Divide => left.wrapping_div(right),
                Modulo => left.wrapping_rem(right),
                BitAnd => left & right,
                BitOr => left | right,
                BitXor => left ^ right,
                ShiftLeft => left.wrapping_shl(right as u32),
                ShiftRight => left.wrapping_shr(right as u32),
                Less => (left < right) as i64,
                Greater => (left > right) as i64,
                LessEqual => (left <= right) as i64,
                GreaterEqual => (left >= right) as i64,
                Equal => (left == right) as i64,
                NotEqual => (left != right) as i64,
                LogicalAnd => (left != 0 && right != 0) as i64,
                LogicalOr => (left != 0 || right != 0) as i64,
            }
        }
        Expression::Conditional { condition, when_true, when_false } => {
            if fold_constant_expression(condition)? != 0 {
                fold_constant_expression(when_true)?
            } else {
                fold_constant_expression(when_false)?
            }
        }
        Expression::Cast { target_type, operand } => {
            let value = fold_constant_expression(operand)?;
            match target_type {
                // A pointer cast keeps the (integer) address value; a non-integer
                // cast cannot be represented here.
                Type::Pointer(_) | Type::StructPointer { .. } => value,
                Type::Float | Type::Double | Type::Struct { .. } | Type::Void => {
                    return Err(Diagnostic::error("a non-integer cast in a constant initializer is not supported yet (roadmap)"))
                }
                integer => truncate_to_integer(value, *integer),
            }
        }
        _ => return Err(Diagnostic::error("a non-constant global initializer is not supported yet (roadmap)")),
    })
}

/// Fold a constant expression to an `f64` for a `float`/`double` global initializer —
/// the arithmetic C evaluates in `double`, and the caller narrows to the global's width
/// (`static float const deg_to_rad = M_PI / 180;`, `1.0f / 3.0f`). Integer literals
/// promote to `f64` (mixed `double / int`); only the operators mwcc constant-folds in a
/// float initializer are handled.
pub(crate) fn fold_constant_float(expression: &Expression) -> Compilation<f64> {
    use BinaryOperator::*;
    Ok(match expression {
        Expression::FloatLiteral(value) => *value,
        Expression::IntegerLiteral(value) => *value as f64,
        Expression::Unary { operator: UnaryOperator::Negate, operand } => -fold_constant_float(operand)?,
        Expression::Binary { operator, left, right } => {
            let left = fold_constant_float(left)?;
            let right = fold_constant_float(right)?;
            match operator {
                Add => left + right,
                Subtract => left - right,
                Multiply => left * right,
                Divide => left / right,
                _ => return Err(Diagnostic::error("unsupported operator in a float constant initializer (roadmap)")),
            }
        }
        _ => return Err(Diagnostic::error("a non-constant float global initializer is not supported yet (roadmap)")),
    })
}

/// Reduce `value` to `integer_type`'s width, sign-extending a signed type — the
/// effect of a C integer cast on a constant.
pub(crate) fn truncate_to_integer(value: i64, integer_type: Type) -> i64 {
    let bits = integer_type.width() as u32;
    if bits >= 64 {
        return value;
    }
    let masked = value & ((1i64 << bits) - 1);
    if integer_type.is_signed() && masked & (1i64 << (bits - 1)) != 0 {
        masked - (1i64 << bits)
    } else {
        masked
    }
}

impl Parser {
    pub(crate) fn expression(&mut self) -> Compilation<Expression> {
        // A compound assignment is valid in expression position too —
        // `(c -= '0') >= base` (strtoul's digit fold). Handled here so every
        // expression() caller (parens, conditions) accepts it.
        let first = self.factor()?;
        if let Some(operator) = self.peek_compound_assignment() {
            self.advance();
            self.advance();
            let rhs = self.expression()?;
            return Ok(Expression::Assign {
                target: Box::new(first.clone()),
                value: Box::new(Expression::Binary {
                    operator,
                    left: Box::new(first),
                    right: Box::new(rhs),
                }),
            });
        }
        let condition = self.binary_expression_from(first, 1)?;
        // ternary conditional has the lowest precedence above assignment
        if *self.peek() == Token::Question {
            self.advance();
            let when_true = self.expression()?;
            self.expect(Token::Colon)?;
            let when_false = self.expression()?;
            return Ok(Expression::Conditional {
                condition: Box::new(condition),
                when_true: Box::new(when_true),
                when_false: Box::new(when_false),
            });
        }
        // assignment is the lowest precedence and right-associative: `a = b = c`.
        if *self.peek() == Token::Equals {
            self.advance();
            let value = self.expression()?;
            return Ok(Expression::Assign { target: Box::new(condition), value: Box::new(value) });
        }
        Ok(condition)
    }

    /// A for-clause element: a compound assignment (`i <<= 1` — statement-only
    /// elsewhere), a plain assignment, or any expression. Mirrors
    /// `parse_simple_statement`'s routing in expression position.
    pub(crate) fn assignment_expression(&mut self) -> Compilation<Expression> {
        let first = self.factor()?;
        if let Some(operator) = self.peek_compound_assignment() {
            self.advance();
            self.advance();
            let rhs = self.expression()?;
            return Ok(Expression::Assign {
                target: Box::new(first.clone()),
                value: Box::new(Expression::Binary {
                    operator,
                    left: Box::new(first),
                    right: Box::new(rhs),
                }),
            });
        }
        if *self.peek() == Token::Equals {
            self.advance();
            let value = self.expression()?;
            return Ok(Expression::Assign { target: Box::new(first), value: Box::new(value) });
        }
        self.binary_expression_from(first, 1)
    }

    /// Precedence-climbing parse of left-associative binary operators with
    /// precedence at least `minimum`.
    pub(crate) fn binary_expression(&mut self, minimum: u8) -> Compilation<Expression> {
        let left = self.factor()?;
        self.binary_expression_from(left, minimum)
    }

    /// The climb continued from an already-parsed left operand.
    pub(crate) fn binary_expression_from(&mut self, mut left: Expression, minimum: u8) -> Compilation<Expression> {
        while let Some(operator) = self.peek_binary_operator() {
            if operator.precedence() < minimum {
                break;
            }
            self.advance();
            let right = self.binary_expression(operator.precedence() + 1)?;
            left = Expression::Binary { operator, left: Box::new(left), right: Box::new(right) };
            // Fold ANY constant operation on two integer literals to its value (`li r3,N`) — mwcc
            // folds all constant subexpressions. Arithmetic/bitwise/shift already lowered to the
            // constant; this also covers division/modulo (else a runtime divide) and the comparison
            // and logical operators (else a runtime compare). Enables `sizeof(a)/sizeof(b)`, `5>3`, etc.
            if let Expression::Binary { left: lhs, right: rhs, .. } = &left {
                if matches!((lhs.as_ref(), rhs.as_ref()), (Expression::IntegerLiteral(_), Expression::IntegerLiteral(_))) {
                    if let Ok(value) = fold_constant_expression(&left) {
                        left = Expression::IntegerLiteral(value);
                    }
                }
            }
        }
        Ok(left)
    }

    pub(crate) fn peek_binary_operator(&self) -> Option<BinaryOperator> {
        Some(match self.peek() {
            Token::Plus => BinaryOperator::Add,
            Token::Minus => BinaryOperator::Subtract,
            Token::Star => BinaryOperator::Multiply,
            Token::Slash => BinaryOperator::Divide,
            Token::Percent => BinaryOperator::Modulo,
            Token::Ampersand => BinaryOperator::BitAnd,
            Token::Pipe => BinaryOperator::BitOr,
            Token::Caret => BinaryOperator::BitXor,
            Token::ShiftLeft => BinaryOperator::ShiftLeft,
            Token::ShiftRight => BinaryOperator::ShiftRight,
            Token::Less => BinaryOperator::Less,
            Token::Greater => BinaryOperator::Greater,
            Token::LessEqual => BinaryOperator::LessEqual,
            Token::GreaterEqual => BinaryOperator::GreaterEqual,
            Token::EqualEqual => BinaryOperator::Equal,
            Token::BangEqual => BinaryOperator::NotEqual,
            Token::AmpersandAmpersand => BinaryOperator::LogicalAnd,
            Token::PipePipe => BinaryOperator::LogicalOr,
            _ => return None,
        })
    }

    pub(crate) fn factor(&mut self) -> Compilation<Expression> {
        // prefix dereference: `*pointer`
        if *self.peek() == Token::Star {
            self.advance();
            let pointer = self.factor()?;
            // `*&x` cancels to `x` — the dereference of an address-of is the operand itself (the C
            // identity). So `**&p` is `*p` and `*&*p = v` is `*p = v`, as mwcc emits.
            if let Expression::AddressOf { operand } = pointer {
                return Ok(*operand);
            }
            return Ok(Expression::Dereference { pointer: Box::new(pointer) });
        }
        // prefix address-of: `&lvalue`
        if *self.peek() == Token::Ampersand {
            self.advance();
            let operand = self.factor()?;
            return Ok(Expression::AddressOf { operand: Box::new(operand) });
        }
        // Unary plus is a no-op: it performs only the integer promotions a read already does, so
        // `+a` is exactly `a` — parse and discard it (mwcc emits identical code). `++` is a distinct
        // `PlusPlus` token handled below, so this never intercepts a pre-increment.
        if *self.peek() == Token::Plus {
            self.advance();
            return self.factor();
        }
        // prefix unary operators
        let unary = match self.peek() {
            Token::Minus => Some(UnaryOperator::Negate),
            Token::Tilde => Some(UnaryOperator::BitNot),
            Token::Bang => Some(UnaryOperator::LogicalNot),
            _ => None,
        };
        if let Some(operator) = unary {
            self.advance();
            let operand = self.factor()?;
            let unary_expression = Expression::Unary { operator, operand: Box::new(operand) };
            // Fold a unary operator on an integer literal (`-5`, `~0xff`, `!0`) to its value, as mwcc
            // does — e.g. `!0` is `li r3,1`, not a runtime `cntlzw` sequence.
            if let Expression::Unary { operand, .. } = &unary_expression {
                if matches!(operand.as_ref(), Expression::IntegerLiteral(_)) {
                    if let Ok(value) = fold_constant_expression(&unary_expression) {
                        return Ok(Expression::IntegerLiteral(value));
                    }
                }
            }
            return Ok(unary_expression);
        }
        // prefix increment/decrement: `++x` / `--x` desugar to `x = x ± 1`. The
        // value of the expression is the assigned (new) value, which an assignment
        // expression already yields — exact for the prefix form.
        let prefix_step = match self.peek() {
            Token::PlusPlus => Some(BinaryOperator::Add),
            Token::MinusMinus => Some(BinaryOperator::Subtract),
            _ => None,
        };
        if let Some(operator) = prefix_step {
            self.advance();
            let operand = self.factor()?;
            return Ok(increment_assignment(operand, operator));
        }

        // `sizeof(type)` is a compile-time constant — the type's byte size as a `size_t`
        // literal, which lowers to `li r3, N`. `struct S` uses its laid-out size, a pointer is
        // 4, scalars are their width in bytes. The expression forms (`sizeof x`, `sizeof(expr)`)
        // need the operand's type and are left to defer.
        if matches!(self.peek(), Token::Identifier(name) if name == "sizeof") {
            self.advance(); // `sizeof`
            let parenthesized = *self.peek() == Token::ParenOpen;
            if parenthesized {
                self.advance(); // `(`
            }
            if parenthesized && self.peek_is_type() {
                let target_type = self.parse_type()?;
                self.expect(Token::ParenClose)?;
                let bytes = match target_type {
                    mwcc_syntax_trees::Type::Struct { size, .. } => size as u32,
                    mwcc_syntax_trees::Type::Pointer(_) | mwcc_syntax_trees::Type::StructPointer { .. } => 4,
                    other => other.width() as u32 / 8,
                };
                return Ok(Expression::IntegerLiteral(bytes as i64));
            }
            // `sizeof expr` / `sizeof(expr)` for a resolvable form folds to a `size_t` constant
            // (`li r3,N`), like `sizeof(type)`: a known variable, a struct member (`s->f`), a cast,
            // or a pointer deref/subscript (`*p`, `a[i]` -> the pointee size). Other shapes defer.
            let operand = if parenthesized {
                let inner = self.expression()?;
                self.expect(Token::ParenClose)?;
                inner
            } else {
                self.factor()?
            };
            // The byte size of a type: a struct uses its laid-out size, a pointer is 4, a scalar is
            // its width/8.
            let size_of = |value_type: Type| -> u32 {
                match value_type {
                    Type::Struct { size, .. } => size as u32,
                    Type::Pointer(_) | Type::StructPointer { .. } => 4,
                    other => other.width() as u32 / 8,
                }
            };
            let bytes = match &operand {
                // A local (parameter/scalar/array) shadows a global of the same name, so consult the
                // per-function maps first, then the file-scope `global_sizes` (total byte size).
                Expression::Variable(name) => self
                    .variable_array_bytes
                    .get(name)
                    .copied()
                    .or_else(|| self.variable_types.get(name).map(|variable_type| size_of(*variable_type)))
                    .or_else(|| self.global_sizes.get(name).map(|&(total, _)| total)),
                Expression::Member { member_type, .. } => Some(size_of(*member_type)),
                Expression::Cast { target_type, .. } => Some(size_of(*target_type)),
                // `*p` / `a[i]`: the size of the pointed-to element. For an ARRAY base the element
                // type is in variable_types (local) or global_sizes (file-scope); for a POINTER base
                // it is the pointee.
                Expression::Dereference { pointer } | Expression::Index { base: pointer, .. } => match pointer.as_ref() {
                    Expression::Variable(name) if self.variable_array_bytes.contains_key(name) => self.variable_types.get(name).map(|element_type| size_of(*element_type)),
                    Expression::Variable(name) if self.variable_types.contains_key(name) => match self.variable_types.get(name) {
                        Some(Type::Pointer(pointee)) => Some(size_of(pointee.element())),
                        Some(Type::StructPointer { element_size }) => Some(*element_size as u32),
                        _ => None,
                    },
                    Expression::Variable(name) => self.global_sizes.get(name).and_then(|&(_, array_element)| array_element),
                    _ => None,
                },
                _ => None,
            };
            if let Some(bytes) = bytes {
                return Ok(Expression::IntegerLiteral(bytes as i64));
            }
            return Err(Diagnostic::error("sizeof of this expression is not supported yet (roadmap)"));
        }

        // A `(struct S *)x` cast carries the struct tag (stashed by `parse_type` in
        // `last_struct_tag`) so a member access on the cast result resolves its layout.
        let mut cast_struct_tag: Option<String> = None;
        let mut expression = match self.advance() {
            Token::IntegerLiteral(value) => Expression::IntegerLiteral(value),
            Token::FloatLiteral(value) => Expression::FloatLiteral(value),
            // A string literal (the raw bytes) — pooled and loaded by address.
            Token::StringLiteral(bytes) => Expression::StringLiteral(bytes),
            // `name(args)` is a call; a bare `name` is a variable.
            Token::Identifier(name) if *self.peek() == Token::ParenOpen => {
                self.advance();
                let mut arguments = Vec::new();
                if *self.peek() != Token::ParenClose {
                    loop {
                        arguments.push(self.expression()?);
                        if *self.peek() == Token::Comma {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
                self.expect(Token::ParenClose)?;
                // A call to a PARSED single-return inline definition substitutes
                // the body (mwcc -inline auto inlines it; a bl would be wrong
                // bytes). Only PURE arguments (a variable or literal) substitute
                // — the body may read a parameter several times; an impure call
                // stays a Call and the skipped-inline check defers the unit.
                match self.inline_bodies.get(&name) {
                    Some((parameters, body))
                        if parameters.len() == arguments.len()
                            && arguments
                                .iter()
                                .all(|argument| matches!(argument, Expression::Variable(_) | Expression::IntegerLiteral(_))) =>
                    {
                        let map: std::collections::HashMap<&str, &Expression> = parameters
                            .iter()
                            .map(String::as_str)
                            .zip(arguments.iter())
                            .collect();
                        substitute_variables(body, &map)
                    }
                    _ => Expression::Call { name, arguments },
                }
            }
            // A bare name is an enumerator (its integer value) if known, else a variable.
            Token::Identifier(name) => match self.enum_constants.get(&name) {
                Some(&value) => Expression::IntegerLiteral(value),
                None => Expression::Variable(name),
            },
            Token::ParenOpen => {
                // `(type) expr` is a cast; otherwise a parenthesised expression.
                if self.peek_is_type() {
                    let mut target_type = self.parse_type()?;
                    // A function-pointer cast `(RET (*)(params))` targets a pointer.
                    if *self.peek() == Token::ParenOpen && self.tokens.get(self.position + 1) == Some(&Token::Star) {
                        self.advance(); // `(`
                        self.advance(); // `*`
                        self.expect(Token::ParenClose)?;
                        self.expect(Token::ParenOpen)?;
                        let mut depth = 1;
                        while depth > 0 {
                            match self.advance() {
                                Token::ParenOpen => depth += 1,
                                Token::ParenClose => depth -= 1,
                                Token::EndOfFile => return Err(Diagnostic::error("unterminated function-pointer cast")),
                                _ => {}
                            }
                        }
                        target_type = mwcc_syntax_trees::Type::Pointer(mwcc_syntax_trees::Pointee::Int);
                    }
                    self.expect(Token::ParenClose)?;
                    // Capture the cast's struct tag before parsing the operand (which may
                    // itself run `parse_type` and overwrite `last_struct_tag`).
                    if matches!(target_type, mwcc_syntax_trees::Type::StructPointer { .. }) {
                        cast_struct_tag = self.last_struct_tag.take();
                    }
                    let operand = self.factor()?;
                    Expression::Cast { target_type, operand: Box::new(operand) }
                } else {
                    // A parenthesized expression may be a comma operator `(a, b, …)`:
                    // each left operand is evaluated for side effects, the last yields
                    // the value. (Call arguments and declarators split on commas at a
                    // lower level, so this only applies inside grouping parens.)
                    let mut inner = self.expression()?;
                    while *self.peek() == Token::Comma {
                        self.advance();
                        let right = self.expression()?;
                        inner = Expression::Comma { left: Box::new(inner), right: Box::new(right) };
                    }
                    self.expect(Token::ParenClose)?;
                    inner
                }
            }
            other => return Err(Diagnostic::error(format!("expected an expression, found {other}"))),
        };
        // postfix subscript `base[index]` and member access `base->field` /
        // `base.field`, left-associative. The struct tag is threaded through the
        // chain so `a->b->c` resolves each `->` in the right struct layout.
        let mut struct_tag = match &expression {
            Expression::Variable(name) => self.variable_structs.get(name).cloned(),
            // `((struct S *)x)->field`: the tag came from the cast's target type — from
            // this factor's own cast, or (via the parens) the inner factor's recorded
            // `expression_struct_tag`.
            Expression::Cast { .. } => cast_struct_tag.take().or_else(|| self.expression_struct_tag.take()),
            // `(*p).field` and `(*(struct S *)x).field`: dereference-then-member is the
            // same access as the arrow form `p->field`, so it carries the pointee's tag —
            // taken from a struct/union-pointer variable, or from the cast recorded in
            // `expression_struct_tag` when the inner operand was parsed.
            Expression::Dereference { pointer } => match pointer.as_ref() {
                Expression::Variable(name) => self.variable_structs.get(name).cloned(),
                _ => self.expression_struct_tag.take(),
            },
            _ => None,
        };
        loop {
            match self.peek() {
                // `(*fp)(args)` — an indirect call through a function-pointer
                // variable. The Call carries the VARIABLE's name; codegen
                // resolves locals/parameters (and defers the unallocated).
                Token::ParenOpen
                    if matches!(&expression, Expression::Dereference { pointer }
                        if matches!(pointer.as_ref(), Expression::Variable(_))) =>
                {
                    let Expression::Dereference { pointer } = &expression else { unreachable!() };
                    let Expression::Variable(name) = pointer.as_ref() else { unreachable!() };
                    let name = name.clone();
                    self.advance(); // `(`
                    let mut arguments = Vec::new();
                    if *self.peek() != Token::ParenClose {
                        loop {
                            arguments.push(self.expression()?);
                            if *self.peek() == Token::Comma {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(Token::ParenClose)?;
                    expression = Expression::Call { name, arguments };
                }
                Token::BracketOpen => {
                    self.advance();
                    let index = self.expression()?;
                    self.expect(Token::BracketClose)?;
                    expression = Expression::Index { base: Box::new(expression), index: Box::new(index) };
                    // Indexing a struct pointer/array yields a struct element of the
                    // same tag (so `a[i].field` resolves); a non-struct base already
                    // carries no tag, so this leaves it `None`.
                }
                Token::Arrow | Token::Dot => {
                    self.advance();
                    let field = self.parse_identifier()?;
                    let tag = struct_tag
                        .take()
                        .ok_or_else(|| Diagnostic::error(format!("member '{field}' on a non-struct-pointer base")))?;
                    let layout = self.structs.get(&tag).ok_or_else(|| Diagnostic::error(format!("struct '{tag}' is not declared")))?;
                    let struct_size = layout.size;
                    let member = layout
                        .fields
                        .get(&field)
                        .ok_or_else(|| Diagnostic::error(format!("struct '{tag}' has no member '{field}'")))?;
                    let bit_field = member.bit_field;
                    let signed = member.member_type.is_signed();
                    let (offset, member_type, next_tag, array_element) =
                        (member.offset, member.member_type, member.struct_tag.clone(), member.array_element);
                    // `a[i].field`: the index scales by the struct size — recorded so
                    // codegen can emit `a + i*size + offset`.
                    let index_stride = matches!(expression, Expression::Index { .. }).then_some(struct_size);
                    // `(*p).field` is exactly `p->field`: the member's base is the pointer
                    // itself, so unwrap one dereference level here (the index_stride check
                    // above already saw the original shape). Without this the base would be
                    // `*p` and codegen would emit a spurious extra load.
                    let mut base_offset = 0u16;
                    let mut base_stride: Option<u16> = None;
                    expression = match expression {
                        Expression::Dereference { pointer } => *pointer,
                        // An EMBEDDED struct-value member folds into its base:
                        // `p->state.eof` is ONE access at offset(state)+offset(eof)
                        // — a struct VALUE member is storage, not a pointer, so no
                        // intermediate load exists.
                        Expression::Member { base, offset: outer_offset, member_type: Type::Struct { .. }, index_stride: outer_stride } => {
                            base_offset = outer_offset;
                            base_stride = outer_stride;
                            *base
                        }
                        other => other,
                    };
                    let offset = offset + base_offset;
                    let index_stride = base_stride.or(index_stride);
                    if let Some((bit_offset, width)) = bit_field {
                        // A bit-field read is the containing unit load shifted+masked to
                        // the field's bits: `(load >> shift) & mask`, which lowers to
                        // mwcc's `lbz/lhz; rlwinm`. The unit load is the smallest byte /
                        // halfword / word span covering the field. Signed bit-fields
                        // (sign-extended) defer until that variant is added.
                        if signed {
                            return Err(Diagnostic::error(format!("a signed bit-field member '{field}' is not supported yet (roadmap)")));
                        }
                        let byte_start = (bit_offset / 8) as u16;
                        let byte_end = ((bit_offset + width - 1) / 8) as u16;
                        let (load_type, load_bits) = match byte_end - byte_start {
                            0 => (Type::UnsignedChar, 8u32),
                            1 => (Type::UnsignedShort, 16),
                            _ => (Type::UnsignedInt, 32),
                        };
                        let shift = load_bits - (bit_offset as u32 - byte_start as u32 * 8) - width as u32;
                        let load = Expression::Member { base: Box::new(expression), offset: offset + byte_start, member_type: load_type, index_stride };
                        let value = if shift > 0 {
                            Expression::Binary { operator: mwcc_syntax_trees::BinaryOperator::ShiftRight, left: Box::new(load), right: Box::new(Expression::IntegerLiteral(shift as i64)) }
                        } else {
                            load
                        };
                        expression = if width as u32 == load_bits {
                            value
                        } else {
                            let mask = (1i64 << width) - 1;
                            Expression::Binary { operator: mwcc_syntax_trees::BinaryOperator::BitAnd, left: Box::new(value), right: Box::new(Expression::IntegerLiteral(mask)) }
                        };
                        struct_tag = None;
                        continue;
                    }
                    expression = match array_element {
                        // An array member decays to the address of its first element.
                        Some(element) => Expression::MemberAddress { base: Box::new(expression), offset, element },
                        None => Expression::Member { base: Box::new(expression), offset, member_type, index_stride },
                    };
                    struct_tag = next_tag;
                }
                _ => break,
            }
        }
        // Record the resolved struct tag so a wrapping `(...)` can recover it (the
        // base of `((struct S *)x)->field` is parsed in a nested `factor`).
        self.expression_struct_tag = struct_tag.clone();
        // postfix increment/decrement: `x++` / `x--`. Represented FAITHFULLY
        // as PostStep (the old value) — statement/step positions discard the
        // value and lower to the Assign desugar at consumption; value
        // positions match the marker or defer (pre/post were previously
        // conflated, a latent wrong-bytes source).
        let postfix_step = match self.peek() {
            Token::PlusPlus => Some(BinaryOperator::Add),
            Token::MinusMinus => Some(BinaryOperator::Subtract),
            _ => None,
        };
        if let Some(operator) = postfix_step {
            self.advance();
            return Ok(Expression::PostStep { target: Box::new(expression), operator });
        }
        Ok(expression)
    }
}

/// Build the `target = target ± 1` assignment that an `++`/`--` desugars to.
fn increment_assignment(target: Expression, operator: BinaryOperator) -> Expression {
    Expression::Assign {
        target: Box::new(target.clone()),
        value: Box::new(Expression::Binary {
            operator,
            left: Box::new(target),
            right: Box::new(Expression::IntegerLiteral(1)),
        }),
    }
}


/// Clone `expression` with every `Variable(name)` in `map` replaced by its
/// argument — the single-return inline substitution.
pub(crate) fn substitute_variables(expression: &Expression, map: &std::collections::HashMap<&str, &Expression>) -> Expression {
    match expression {
        Expression::Variable(name) => match map.get(name.as_str()) {
            Some(&replacement) => replacement.clone(),
            None => expression.clone(),
        },
        Expression::Binary { operator, left, right } => Expression::Binary {
            operator: *operator,
            left: Box::new(substitute_variables(left, map)),
            right: Box::new(substitute_variables(right, map)),
        },
        Expression::Unary { operator, operand } => Expression::Unary {
            operator: *operator,
            operand: Box::new(substitute_variables(operand, map)),
        },
        Expression::Cast { target_type, operand } => Expression::Cast {
            target_type: *target_type,
            operand: Box::new(substitute_variables(operand, map)),
        },
        Expression::Dereference { pointer } => Expression::Dereference {
            pointer: Box::new(substitute_variables(pointer, map)),
        },
        Expression::AddressOf { operand } => Expression::AddressOf {
            operand: Box::new(substitute_variables(operand, map)),
        },
        Expression::Index { base, index } => Expression::Index {
            base: Box::new(substitute_variables(base, map)),
            index: Box::new(substitute_variables(index, map)),
        },
        Expression::Member { base, offset, member_type, index_stride } => Expression::Member {
            base: Box::new(substitute_variables(base, map)),
            offset: *offset,
            member_type: *member_type,
            index_stride: *index_stride,
        },
        Expression::MemberAddress { base, offset, element } => Expression::MemberAddress {
            base: Box::new(substitute_variables(base, map)),
            offset: *offset,
            element: *element,
        },
        Expression::Conditional { condition, when_true, when_false } => Expression::Conditional {
            condition: Box::new(substitute_variables(condition, map)),
            when_true: Box::new(substitute_variables(when_true, map)),
            when_false: Box::new(substitute_variables(when_false, map)),
        },
        Expression::Call { name, arguments } => Expression::Call {
            name: name.clone(),
            arguments: arguments.iter().map(|argument| substitute_variables(argument, map)).collect(),
        },
        Expression::Assign { target, value } => Expression::Assign {
            target: Box::new(substitute_variables(target, map)),
            value: Box::new(substitute_variables(value, map)),
        },
        other => other.clone(),
    }
}
