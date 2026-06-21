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
                Type::Pointer(_) | Type::StructPointer => value,
                Type::Float | Type::Double | Type::Struct { .. } | Type::Void => {
                    return Err(Diagnostic::error("a non-integer cast in a constant initializer is not supported yet (roadmap)"))
                }
                integer => truncate_to_integer(value, *integer),
            }
        }
        _ => return Err(Diagnostic::error("a non-constant global initializer is not supported yet (roadmap)")),
    })
}

/// Reduce `value` to `integer_type`'s width, sign-extending a signed type — the
/// effect of a C integer cast on a constant.
fn truncate_to_integer(value: i64, integer_type: Type) -> i64 {
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
        let condition = self.binary_expression(1)?;
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

    /// Precedence-climbing parse of left-associative binary operators with
    /// precedence at least `minimum`.
    pub(crate) fn binary_expression(&mut self, minimum: u8) -> Compilation<Expression> {
        let mut left = self.factor()?;
        while let Some(operator) = self.peek_binary_operator() {
            if operator.precedence() < minimum {
                break;
            }
            self.advance();
            let right = self.binary_expression(operator.precedence() + 1)?;
            left = Expression::Binary { operator, left: Box::new(left), right: Box::new(right) };
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
            return Ok(Expression::Dereference { pointer: Box::new(pointer) });
        }
        // prefix address-of: `&lvalue`
        if *self.peek() == Token::Ampersand {
            self.advance();
            let operand = self.factor()?;
            return Ok(Expression::AddressOf { operand: Box::new(operand) });
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
            return Ok(Expression::Unary { operator, operand: Box::new(operand) });
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
                Expression::Call { name, arguments }
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
                    if target_type == mwcc_syntax_trees::Type::StructPointer {
                        cast_struct_tag = self.last_struct_tag.take();
                    }
                    let operand = self.factor()?;
                    Expression::Cast { target_type, operand: Box::new(operand) }
                } else {
                    let inner = self.expression()?;
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
            _ => None,
        };
        loop {
            match self.peek() {
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
                    let (offset, member_type, next_tag, array_element) =
                        (member.offset, member.member_type, member.struct_tag.clone(), member.array_element);
                    // `a[i].field`: the index scales by the struct size — recorded so
                    // codegen can emit `a + i*size + offset`.
                    let index_stride = matches!(expression, Expression::Index { .. }).then_some(struct_size);
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
        // postfix increment/decrement: `x++` / `x--`. Desugared to `x = x ± 1` like
        // the prefix form — the post-value (old value) is not modeled, so a use of
        // the result is approximate; the common loop-step / statement positions
        // discard it, where the two forms coincide.
        let postfix_step = match self.peek() {
            Token::PlusPlus => Some(BinaryOperator::Add),
            Token::MinusMinus => Some(BinaryOperator::Subtract),
            _ => None,
        };
        if let Some(operator) = postfix_step {
            self.advance();
            return Ok(increment_assignment(expression, operator));
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
