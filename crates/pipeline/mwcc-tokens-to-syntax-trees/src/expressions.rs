//! Precedence-climbing expression parsing: ternary, binary operators, prefix
//! unary operators, casts, and primary factors.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{BinaryOperator, ConditionalOrigin, Expression, Type, UnaryOperator};
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
        Expression::Binary {
            operator,
            left,
            right,
        } => {
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
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            ..
        } => {
            if fold_constant_expression(condition)? != 0 {
                fold_constant_expression(when_true)?
            } else {
                fold_constant_expression(when_false)?
            }
        }
        Expression::Cast {
            target_type,
            operand,
        } => {
            match target_type {
                // A pointer cast keeps the (integer) address value; a non-integer
                // cast cannot be represented here.
                Type::Pointer(_) | Type::StructPointer { .. } => {
                    fold_constant_expression(operand)?
                }
                Type::Float | Type::Double | Type::Struct { .. } | Type::Void => {
                    return Err(Diagnostic::error("a non-integer cast in a constant initializer is not supported yet (roadmap)"))
                }
                integer => {
                    // An explicit integer cast makes an otherwise-floating
                    // constant expression valid in an integer initializer:
                    // `(s16)(90.0f * (65536.0f / 360.0f))`. C truncates toward
                    // zero before applying the destination width/sign.
                    let value = match fold_constant_expression(operand) {
                        Ok(value) => value,
                        Err(_) => fold_constant_float(operand)?.trunc() as i64,
                    };
                    truncate_to_integer(value, *integer)
                }
            }
        }
        _ => {
            return Err(Diagnostic::error(format!(
                "a non-constant global initializer is not supported yet (roadmap): {expression:?}"
            )))
        }
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
        Expression::Unary {
            operator: UnaryOperator::Negate,
            operand,
        } => -fold_constant_float(operand)?,
        Expression::Binary {
            operator,
            left,
            right,
        } => {
            let left = fold_constant_float(left)?;
            let right = fold_constant_float(right)?;
            match operator {
                Add => left + right,
                Subtract => left - right,
                Multiply => left * right,
                // mwcc folds 0.0/0.0 to the PowerPC HARDWARE NaN — sign bit
                // SET (melee math.c's `float_nan = 0.0 / 0.0` images FFC00000).
                Divide if left == 0.0 && right == 0.0 => f64::from_bits(0xFFF8_0000_0000_0000),
                Divide => left / right,
                _ => {
                    return Err(Diagnostic::error(
                        "unsupported operator in a float constant initializer (roadmap)",
                    ))
                }
            }
        }
        Expression::Cast {
            target_type: Type::Float | Type::Double,
            operand,
        } => fold_constant_float(operand)?,
        Expression::Cast {
            target_type,
            operand,
        } if matches!(
            target_type,
            Type::Int
                | Type::UnsignedInt
                | Type::Char
                | Type::UnsignedChar
                | Type::Short
                | Type::UnsignedShort
                | Type::LongLong
                | Type::UnsignedLongLong
        ) =>
        {
            fold_constant_expression(operand)? as f64
        }
        _ => {
            return Err(Diagnostic::error(
                "a non-constant float global initializer is not supported yet (roadmap)",
            ))
        }
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
    fn sizeof_type_bytes(value_type: Type) -> u32 {
        match value_type {
            Type::Struct { size, .. } => size,
            Type::Pointer(_) | Type::StructPointer { .. } => 4,
            other => other.width() as u32 / 8,
        }
    }

    /// Resolve an expression's byte size without evaluating it. `sizeof` needs
    /// this small type query to follow pointer provenance through members and
    /// array decay, where a variable-only lookup loses the pointee type.
    fn sizeof_expression_bytes(&self, expression: &Expression) -> Option<u32> {
        match expression {
            // A local shadows a same-named global. Arrays report their complete
            // storage here; subscripting is handled by pointed_element_bytes.
            Expression::Variable(name) => self
                .variable_array_bytes
                .get(name)
                .copied()
                .or_else(|| {
                    self.variable_types
                        .get(name)
                        .map(|value_type| Self::sizeof_type_bytes(*value_type))
                })
                .or_else(|| self.global_sizes.get(name).map(|&(total, _)| total)),
            Expression::Member { member_type, .. } => Some(Self::sizeof_type_bytes(*member_type)),
            // A bare array member reports the whole member, while a subscript
            // through it asks pointed_element_bytes for one element.
            Expression::MemberAddress { .. } => self.last_member_array_bytes.map(u32::from),
            Expression::Cast { target_type, .. } => Some(Self::sizeof_type_bytes(*target_type)),
            Expression::Dereference { pointer } | Expression::Index { base: pointer, .. } => {
                self.pointed_element_bytes(pointer)
            }
            _ => None,
        }
    }

    fn pointed_element_bytes(&self, pointer: &Expression) -> Option<u32> {
        match pointer {
            Expression::Variable(name) if self.variable_array_bytes.contains_key(name) => self
                .variable_types
                .get(name)
                .map(|element| Self::sizeof_type_bytes(*element)),
            Expression::Variable(name) if self.variable_types.contains_key(name) => {
                match self.variable_types.get(name) {
                    Some(Type::Pointer(pointee)) => {
                        Some(Self::sizeof_type_bytes(pointee.element()))
                    }
                    Some(Type::StructPointer { element_size }) => Some(*element_size),
                    _ => None,
                }
            }
            Expression::Variable(name) => self
                .global_sizes
                .get(name)
                .and_then(|&(_, array_element)| array_element)
                .or_else(|| match self.global_types.get(name) {
                    Some(Type::Pointer(pointee)) => {
                        Some(Self::sizeof_type_bytes(pointee.element()))
                    }
                    Some(Type::StructPointer { element_size }) => Some(*element_size),
                    _ => None,
                }),
            Expression::Member { member_type, .. }
            | Expression::Cast {
                target_type: member_type,
                ..
            } => match member_type {
                Type::Pointer(pointee) => Some(Self::sizeof_type_bytes(pointee.element())),
                Type::StructPointer { element_size } => Some(*element_size),
                _ => None,
            },
            Expression::MemberAddress { element, .. } => {
                Some(Self::sizeof_type_bytes(element.element()))
            }
            Expression::AddressOf { operand } => self.sizeof_expression_bytes(operand),
            _ => None,
        }
    }

    /// Resolve a bare name through the active block-scope shadow renames
    /// (innermost wins); names with no active shadow pass through.
    pub(crate) fn resolve_block_rename(&self, name: String) -> String {
        for (source, internal) in self.block_renames.iter().rev() {
            if *source == name {
                return internal.clone();
            }
        }
        name
    }

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
            // A COMPILE-TIME-CONSTANT condition selects its branch at parse
            // time — fdlibm's `(sizeof(x) == 8 ? *(1+(_INT32*)&x) : ...)`
            // (the __HI/__LO macros) folds to the taken word access, which
            // also makes the LVALUE form a plain dereference store target.
            // (Fire 524: reintroduced WITH the coordinated capture re-bake.)
            if let Ok(value) = fold_constant_expression(&condition) {
                return Ok(if value != 0 { when_true } else { when_false });
            }
            return Ok(Expression::Conditional {
                condition: Box::new(condition),
                when_true: Box::new(when_true),
                when_false: Box::new(when_false),
                origin: ConditionalOrigin::Ternary,
            });
        }
        // assignment is the lowest precedence and right-associative: `a = b = c`.
        if *self.peek() == Token::Equals {
            self.advance();
            let value = self.expression()?;
            return Ok(Expression::Assign {
                target: Box::new(condition),
                value: Box::new(value),
            });
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
            return Ok(Expression::Assign {
                target: Box::new(first),
                value: Box::new(value),
            });
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
    pub(crate) fn binary_expression_from(
        &mut self,
        mut left: Expression,
        minimum: u8,
    ) -> Compilation<Expression> {
        while let Some(operator) = self.peek_binary_operator() {
            if operator.precedence() < minimum {
                break;
            }
            self.advance();
            let right = self.binary_expression(operator.precedence() + 1)?;
            left = Expression::Binary {
                operator,
                left: Box::new(left),
                right: Box::new(right),
            };
            // Fold ANY constant operation on two integer literals to its value (`li r3,N`) — mwcc
            // folds all constant subexpressions. Arithmetic/bitwise/shift already lowered to the
            // constant; this also covers division/modulo (else a runtime divide) and the comparison
            // and logical operators (else a runtime compare). Enables `sizeof(a)/sizeof(b)`, `5>3`, etc.
            if let Expression::Binary {
                left: lhs,
                right: rhs,
                ..
            } = &left
            {
                if matches!(
                    (lhs.as_ref(), rhs.as_ref()),
                    (Expression::IntegerLiteral(_), Expression::IntegerLiteral(_))
                ) {
                    if let Ok(value) = fold_constant_expression(&left) {
                        left = Expression::IntegerLiteral(value);
                    }
                } else if matches!(
                    (lhs.as_ref(), rhs.as_ref()),
                    (Expression::FloatLiteral(_), Expression::FloatLiteral(_))
                        | (Expression::FloatLiteral(_), Expression::IntegerLiteral(_))
                        | (Expression::IntegerLiteral(_), Expression::FloatLiteral(_))
                ) {
                    // Ordinary function expressions get the same arithmetic
                    // constant folding as float globals. Keeping this in the
                    // parser means codegen only sees the literal mwcc pools,
                    // rather than a synthetic runtime divide tree.
                    if let Ok(value) = fold_constant_float(&left) {
                        left = Expression::FloatLiteral(value);
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
            return Ok(Expression::Dereference {
                pointer: Box::new(pointer),
            });
        }
        // prefix address-of: `&lvalue`
        if *self.peek() == Token::Ampersand {
            self.advance();
            let operand = self.factor()?;
            return Ok(Expression::AddressOf {
                operand: Box::new(operand),
            });
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
            let unary_expression = Expression::Unary {
                operator,
                operand: Box::new(operand),
            };
            // Fold a unary operator on an integer literal (`-5`, `~0xff`, `!0`) to its value, as mwcc
            // does — e.g. `!0` is `li r3,1`, not a runtime `cntlzw` sequence.
            if let Expression::Unary { operand, .. } = &unary_expression {
                if matches!(operand.as_ref(), Expression::IntegerLiteral(_)) {
                    if let Ok(value) = fold_constant_expression(&unary_expression) {
                        return Ok(Expression::IntegerLiteral(value));
                    }
                } else if operator == UnaryOperator::Negate
                    && matches!(operand.as_ref(), Expression::FloatLiteral(_))
                {
                    if let Ok(value) = fold_constant_float(&unary_expression) {
                        return Ok(Expression::FloatLiteral(value));
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

        // A C++ named static cast has the same expression representation as a
        // C-style cast once its destination type is known. Keep the syntax in
        // the parser rather than teaching later lowering stages about two
        // spellings of the same conversion.
        if self.cplusplus && matches!(self.peek(), Token::Identifier(name) if name == "static_cast")
        {
            self.advance();
            self.expect(Token::Less)?;
            let target_type = self.parse_type()?;
            self.expect(Token::Greater)?;
            self.expect(Token::ParenOpen)?;
            let operand = self.expression()?;
            self.expect(Token::ParenClose)?;
            return Ok(Expression::Cast {
                target_type,
                operand: Box::new(operand),
            });
        }

        // `_var_arg_typeof(type)` is an mwcc intrinsic: the EABI vararg class
        // code fed to `__va_arg` (measured GC/2.6: aggregate -> 0, gpr scalar/
        // pointer -> 1, long long pair -> 2, float/double -> 3).
        if matches!(self.peek(), Token::Identifier(name) if name == "_var_arg_typeof") {
            self.advance();
            self.expect(Token::ParenOpen)?;
            let target_type = self.parse_type()?;
            // A POINTER type argument (`_var_arg_typeof(wchar_t*)` — printf's
            // %ls arm): stars make it a gpr pointer, class 1.
            let mut starred = false;
            while self.eat_keyword(Token::Star) {
                starred = true;
            }
            self.expect(Token::ParenClose)?;
            let code = if starred {
                1
            } else {
                match target_type {
                    Type::Struct { .. } => 0,
                    Type::LongLong | Type::UnsignedLongLong => 2,
                    Type::Float | Type::Double => 3,
                    _ => 1,
                }
            };
            return Ok(Expression::IntegerLiteral(code));
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
                    mwcc_syntax_trees::Type::Pointer(_)
                    | mwcc_syntax_trees::Type::StructPointer { .. } => 4,
                    other => other.width() as u32 / 8,
                };
                return Ok(Expression::IntegerLiteral(bytes as i64));
            }
            // `sizeof expr` / `sizeof(expr)` for a resolvable form folds to a `size_t` constant
            // (`li r3,N`), like `sizeof(type)`: a known variable, a struct member (`s->f`), a cast,
            // or a pointer deref/subscript (`*p`, `a[i]` -> the pointee size). Other shapes defer.
            self.last_member_array_bytes = None;
            let operand = if parenthesized {
                let inner = self.expression()?;
                self.expect(Token::ParenClose)?;
                inner
            } else {
                self.factor()?
            };
            let bytes = self.sizeof_expression_bytes(&operand);
            if let Some(bytes) = bytes {
                return Ok(Expression::IntegerLiteral(bytes as i64));
            }
            return Err(Diagnostic::error(format!(
                "sizeof of this expression is not supported yet (roadmap): {operand:?}"
            )));
        }

        // A `(struct S *)x` cast carries the struct tag (stashed by `parse_type` in
        // `last_struct_tag`) so a member access on the cast result resolves its layout.
        let mut cast_struct_tag: Option<String> = None;
        let mut expression = match self.advance() {
            Token::IntegerLiteral(value) => Expression::IntegerLiteral(value),
            Token::FloatLiteral(value) => Expression::FloatLiteral(value),
            // A string literal (the raw bytes) — pooled and loaded by address.
            Token::StringLiteral(bytes) => Expression::StringLiteral(bytes),
            // C++ boolean literals are integral constant expressions with the
            // normalized values one and zero. The lexer deliberately keeps
            // them as identifiers so C mode may still use either spelling as
            // an ordinary name.
            Token::Identifier(name) if self.cplusplus && name == "true" => {
                Expression::IntegerLiteral(1)
            }
            Token::Identifier(name) if self.cplusplus && name == "false" => {
                Expression::IntegerLiteral(0)
            }
            Token::Identifier(name) if self.cplusplus && name == "new" => {
                self.parse_cxx_new_expression()?
            }
            // A qualified static member has no implicit `this`. A following
            // argument list is a call whose declaration supplies overload
            // information; a bare member is a data object and uses the same
            // class/namespace encoding without a function suffix.
            Token::Identifier(scope)
                if *self.peek() == Token::Colon && *self.peek_at(1) == Token::Colon =>
            {
                self.advance();
                self.advance();
                let member = self.parse_identifier()?;
                if *self.peek() == Token::ParenOpen {
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
                    if arguments.is_empty()
                        && (self.is_empty_nested_type_constructor(&scope, &member)
                            || self.is_empty_qualified_type_constructor(&scope, &member))
                    {
                        Expression::AggregateLiteral(Vec::new())
                    } else {
                        let name = if self.cxx_namespaces.contains(&scope) {
                            self.resolve_qualified_free_cxx_call(
                                &scope,
                                &member,
                                &arguments,
                            )?
                            .ok_or_else(|| {
                                Diagnostic::error(format!(
                                    "C++ namespace function call '{scope}::{member}' is unavailable (roadmap)"
                                ))
                            })?
                        } else if let Some(name) = self
                            .resolve_explicit_instance_member_call(
                                &scope,
                                &member,
                                arguments.len(),
                            )?
                        {
                            arguments.insert(0, Expression::Variable("this".to_owned()));
                            name
                        } else {
                            self.resolve_static_member_call(&scope, &member, arguments.len())?
                        };
                        Expression::Call { name, arguments }
                    }
                } else {
                    Expression::Variable(
                        self.mangle_data_member_in_current_namespace(&scope, &member)?,
                    )
                }
            }
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
                if let Some(member_call) =
                    self.resolve_implicit_member_call(&name, arguments.len())?
                {
                    match member_call {
                        crate::cxx::ImplicitMemberCall::Direct {
                            name: mangled,
                            is_inline,
                            this_adjustment,
                        } => {
                            if is_inline {
                                // The declaration pass records in-class bodies as
                                // analysis-only skipped definitions. Keep a real
                                // call node here so the semantic inliner can claim
                                // it; codegen still rejects any unexpanded call to
                                // this non-emitted symbol.
                                self.skipped_inline_names.insert(mangled.clone());
                            }
                            let this = if this_adjustment == 0 {
                                Expression::Variable("this".to_string())
                            } else {
                                Expression::MemberAddress {
                                    base: Box::new(Expression::Variable("this".to_string())),
                                    offset: this_adjustment,
                                    element: mwcc_syntax_trees::Pointee::UnsignedChar,
                                    index_stride: None,
                                }
                            };
                            arguments.insert(0, this);
                            Expression::Call {
                                name: mangled,
                                arguments,
                            }
                        }
                        crate::cxx::ImplicitMemberCall::Virtual {
                            dispatch,
                            this_adjustment,
                        } => {
                            let object = if this_adjustment == 0 {
                                Expression::Variable("this".to_string())
                            } else {
                                Expression::MemberAddress {
                                    base: Box::new(Expression::Variable("this".to_string())),
                                    offset: this_adjustment,
                                    element: mwcc_syntax_trees::Pointee::UnsignedChar,
                                    index_stride: None,
                                }
                            };
                            Expression::VirtualCall {
                                object: Box::new(object),
                                vptr_offset: dispatch.vptr_offset,
                                slot_offset: dispatch.slot_offset,
                                return_type: dispatch.return_type,
                                variadic: dispatch.variadic,
                                arguments,
                            }
                        }
                    }
                } else {
                    // A call to a PARSED single-return inline definition substitutes
                    // the body (mwcc -inline auto inlines it; a bl would be wrong
                    // bytes). Stable values may be repeated. A read expression may
                    // also substitute when the body evaluates that parameter exactly
                    // once and unconditionally; this covers pointer accessors such as
                    // `get_user_data(fp->victim)` without duplicating or dropping a
                    // memory read. Everything else stays a Call so the skipped-inline
                    // check can defer the unit safely.
                    match self.inline_bodies.get(&name) {
                        Some((parameters, body))
                            if parameters.len() == arguments.len()
                                && inline_arguments_are_safe(parameters, body, &arguments) =>
                        {
                            let map: std::collections::HashMap<&str, &Expression> = parameters
                                .iter()
                                .map(String::as_str)
                                .zip(arguments.iter())
                                .collect();
                            self.inline_substitution_count += 1;
                            substitute_variables(body, &map)
                        }
                        _ => {
                            let name = if self.cplusplus {
                                self.resolve_free_cxx_call(&name, &arguments)?
                                    .unwrap_or(name)
                            } else {
                                name
                            };
                            Expression::Call { name, arguments }
                        }
                    }
                }
            }
            // A FIXED-ADDRESS global (`AT_ADDRESS`) aliases a const-address deref `*(Type *)addr`;
            // desugar so a following `.member` resolves via the const-address path (byte-exact).
            // `expression_struct_tag` carries the pointee's tag for the postfix member resolver.
            Token::Identifier(name) if self.fixed_address_globals.contains_key(&name) => {
                let (address, cast_target, tag) =
                    self.fixed_address_globals.get(&name).cloned().unwrap();
                self.expression_struct_tag = tag;
                Expression::Dereference {
                    pointer: Box::new(Expression::Cast {
                        target_type: cast_target,
                        operand: Box::new(Expression::IntegerLiteral(address)),
                    }),
                }
            }
            // A bare name is an enumerator (its integer value) if known, else a
            // variable — resolved through any active block-scope shadow renames.
            Token::Identifier(name) => {
                let resolved = self.resolve_block_rename(name.clone());
                if self.variable_types.contains_key(&resolved) || resolved != name {
                    Expression::Variable(resolved)
                } else if let Some(member) = self
                    .current_member_scope
                    .as_deref()
                    .and_then(|scope| self.structs.get(scope))
                    .and_then(|layout| layout.fields.get(&name))
                {
                    // An unqualified data member is rooted at `this`, but its
                    // own aggregate identity—not a stale tag from an earlier
                    // expression—must seed a following `.field`/`->field`.
                    self.expression_struct_tag = member.struct_tag.clone();
                    Expression::Member {
                        base: Box::new(Expression::Variable("this".to_string())),
                        offset: member.offset,
                        member_type: member.member_type,
                        index_stride: None,
                    }
                } else if let Some(mangled) = self.resolve_implicit_static_data_member(&name)? {
                    Expression::Variable(mangled)
                } else if let Some(&value) = self.enum_constants.get(&name) {
                    Expression::IntegerLiteral(value)
                } else {
                    Expression::Variable(resolved)
                }
            }
            Token::ParenOpen => {
                // `(type) expr` is a cast; otherwise a parenthesised expression.
                if self.peek_is_type() {
                    let mut target_type = self.parse_type()?;
                    // A function-pointer cast `(RET (*)(params))` targets a pointer.
                    if *self.peek() == Token::ParenOpen
                        && self.tokens.get(self.position + 1) == Some(&Token::Star)
                    {
                        self.advance(); // `(`
                        self.advance(); // `*`
                        self.expect(Token::ParenClose)?;
                        self.expect(Token::ParenOpen)?;
                        let mut depth = 1;
                        while depth > 0 {
                            match self.advance() {
                                Token::ParenOpen => depth += 1,
                                Token::ParenClose => depth -= 1,
                                Token::EndOfFile => {
                                    return Err(Diagnostic::error(
                                        "unterminated function-pointer cast",
                                    ))
                                }
                                _ => {}
                            }
                        }
                        target_type =
                            mwcc_syntax_trees::Type::Pointer(mwcc_syntax_trees::Pointee::Int);
                    }
                    // Extra stars past parse_type's one (`(wchar_t**)` — printf's
                    // %ls arm): a pointer-to-pointer cast is a word pointer.
                    while self.eat_keyword(Token::Star) {
                        target_type =
                            mwcc_syntax_trees::Type::Pointer(mwcc_syntax_trees::Pointee::Pointer);
                    }
                    self.expect(Token::ParenClose)?;
                    // A COMPOUND LITERAL `(GXColor){ 0, 0, 0xE2, 0x58 }` — a brace
                    // list after a struct-typed cast: serialize the constant image at
                    // parse time (the layout lives here). A relocated element defers.
                    if *self.peek() == Token::BraceOpen {
                        if let (mwcc_syntax_trees::Type::Struct { .. }, Some(tag)) =
                            (target_type, self.last_struct_tag.clone())
                        {
                            let mut relocations = Vec::new();
                            let bytes =
                                self.parse_one_struct_relocated(&tag, 0, &mut relocations)?;
                            if !relocations.is_empty() {
                                return Err(Diagnostic::error(
                                    "a relocated compound literal is not supported yet (roadmap)",
                                ));
                            }
                            self.last_struct_tag = None;
                            return Ok(Expression::CompoundLiteral {
                                struct_tag: tag,
                                bytes,
                            });
                        }
                        return Err(Diagnostic::error(
                            "a non-struct compound literal is not supported yet (roadmap)",
                        ));
                    }
                    // Capture the cast's struct tag before parsing the operand (which may
                    // itself run `parse_type` and overwrite `last_struct_tag`).
                    if matches!(target_type, mwcc_syntax_trees::Type::StructPointer { .. }) {
                        cast_struct_tag = self.last_struct_tag.take();
                    }
                    let operand = self.factor()?;
                    Expression::Cast {
                        target_type,
                        operand: Box::new(operand),
                    }
                } else {
                    // A parenthesized expression may be a comma operator `(a, b, …)`:
                    // each left operand is evaluated for side effects, the last yields
                    // the value. (Call arguments and declarators split on commas at a
                    // lower level, so this only applies inside grouping parens.)
                    let mut inner = self.expression()?;
                    while *self.peek() == Token::Comma {
                        self.advance();
                        let right = self.expression()?;
                        inner = Expression::Comma {
                            left: Box::new(inner),
                            right: Box::new(right),
                        };
                    }
                    self.expect(Token::ParenClose)?;
                    inner
                }
            }
            other => {
                let token_index = self.position.saturating_sub(1);
                if std::env::var_os("MWCC_PARSE_DEBUG").is_some() {
                    let start = token_index.saturating_sub(8);
                    let end = (token_index + 9).min(self.tokens.len());
                    eprintln!(
                        "parse context at token {token_index}: {:?}",
                        &self.tokens[start..end]
                    );
                }
                return Err(Diagnostic::error(format!(
                    "expected an expression, found {other} at {}",
                    self.diagnostic_position(token_index)
                )));
            }
        };
        // postfix subscript `base[index]` and member access `base->field` /
        // `base.field`, left-associative. The struct tag is threaded through the
        // chain so `a->b->c` resolves each `->` in the right struct layout.
        let mut struct_tag = match &expression {
            Expression::Variable(name) => self
                .variable_structs
                .get(name)
                .or_else(|| self.global_structs.get(name))
                .cloned(),
            // `((struct S *)x)->field`: the tag came from the cast's target type — from
            // this factor's own cast, or (via the parens) the inner factor's recorded
            // `expression_struct_tag`.
            Expression::Cast { .. } => cast_struct_tag
                .take()
                .or_else(|| self.expression_struct_tag.take()),
            // `(*p).field` and `(*(struct S *)x).field`: dereference-then-member is the
            // same access as the arrow form `p->field`, so it carries the pointee's tag —
            // taken from a struct/union-pointer variable, or from the cast recorded in
            // `expression_struct_tag` when the inner operand was parsed.
            Expression::Dereference { pointer } => match pointer.as_ref() {
                Expression::Variable(name) => self.variable_structs.get(name).cloned(),
                _ => self.expression_struct_tag.take(),
            },
            // `(chain->member)->field` / `(a[i])->field`: the parenthesized base is a
            // completed member/index chain — its final tag was recorded when the inner
            // factor finished (alloc.c's block_->client_size_ chains).
            Expression::Member { .. } | Expression::Index { .. } => {
                self.expression_struct_tag.take()
            }
            // `(&p->embedded)->field`: the inner member access resolved `embedded`'s
            // aggregate tag before address-of wrapped it. Address-of changes a struct value into
            // a pointer to that same struct, so preserve the recorded tag for the next `->`.
            Expression::AddressOf { .. } => self.expression_struct_tag.take(),
            // `get()->field`: a call to a function that RETURNS a struct pointer carries the
            // pointee's tag (recorded from the `struct S *get(...)` declaration).
            Expression::Call { name, .. } => self.function_return_structs.get(name).cloned(),
            _ => None,
        };
        loop {
            match self.peek() {
                // `(*fp)(args)` — an indirect call through a function-pointer
                // variable. The Call carries the VARIABLE's name; codegen
                // resolves locals/parameters (and defers the unallocated).
                // `(iswspace)(args)` — a parenthesized DIRECT callee. The parens
                // produce a bare Variable; the postfix call binds by name exactly
                // like the unparenthesized spelling (strikers wscanf).
                Token::ParenOpen if matches!(&expression, Expression::Variable(_)) => {
                    let Expression::Variable(name) = &expression else {
                        unreachable!()
                    };
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
                Token::ParenOpen
                    if matches!(&expression, Expression::Dereference { pointer }
                        if matches!(pointer.as_ref(), Expression::Variable(_))) =>
                {
                    let Expression::Dereference { pointer } = &expression else {
                        unreachable!()
                    };
                    let Expression::Variable(name) = pointer.as_ref() else {
                        unreachable!()
                    };
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
                // `base->fp(args)` / `base.fp(args)` — an indirect call through
                // a function-pointer MEMBER (buffer_io's writeFunc). Also the
                // `(*s->fp)(args)` spelling, which parses to Dereference(Member),
                // and the double-deref `(**pp)(args)` — Dereference(Dereference(..)) —
                // whose callee address is the inner pointer value `*pp` (executor's
                // `(**ctors)()` C++ ctor/dtor runner).
                Token::ParenOpen
                    if matches!(
                        &expression,
                        Expression::Member { .. }
                            | Expression::Index { .. }
                            | Expression::Cast { .. }
                    ) || matches!(&expression, Expression::Dereference { pointer }
                            if matches!(pointer.as_ref(), Expression::Member { .. } | Expression::Index { .. } | Expression::Cast { .. } | Expression::Dereference { .. })) =>
                {
                    let target = match expression {
                        Expression::Dereference { pointer } => pointer,
                        other => Box::new(other),
                    };
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
                    expression = Expression::CallThrough { target, arguments };
                }
                Token::BracketOpen => {
                    // A decayed array-typedef / row-pointer parameter (`Mtx m`): the ONLY
                    // modeled subscript form is `m[c1][c2]` with both indices constant,
                    // which desugars to the strided Member access `m + c1*stride + c2*elem`
                    // (the same AST as `p->field`, so all member codegen applies). Every
                    // other form — a single subscript (a row VALUE, whose stride the plain
                    // Index would get wrong), a variable index — errors (defers) instead of
                    // falling through to the wrong-stride generic Index.
                    if let Expression::Variable(name) = &expression {
                        if let Some(&(element, stride)) = self.decayed_row_pointers.get(name) {
                            self.advance(); // `[`
                            let row = self.expression()?;
                            self.expect(Token::BracketClose)?;
                            if *self.peek() != Token::BracketOpen {
                                return Err(Diagnostic::error("a single subscript on an array-typedef parameter is not supported yet (roadmap)"));
                            }
                            self.advance(); // `[`
                            let column = self.expression()?;
                            self.expect(Token::BracketClose)?;
                            let (
                                Expression::IntegerLiteral(row),
                                Expression::IntegerLiteral(column),
                            ) = (&row, &column)
                            else {
                                return Err(Diagnostic::error("a variable subscript on an array-typedef parameter is not supported yet (roadmap)"));
                            };
                            let element_bytes = element.width() as i64 / 8;
                            let offset = row * stride as i64 + column * element_bytes;
                            if offset < 0 || offset > u32::MAX as i64 {
                                return Err(Diagnostic::error(
                                    "an array-typedef subscript offset is out of range",
                                ));
                            }
                            expression = Expression::Member {
                                base: Box::new(expression),
                                offset: offset as u32,
                                member_type: element,
                                index_stride: None,
                            };
                            continue;
                        }
                    }
                    self.advance();
                    let index = self.expression()?;
                    self.expect(Token::BracketClose)?;
                    expression = Expression::Index {
                        base: Box::new(expression),
                        index: Box::new(index),
                    };
                    // Indexing a struct pointer/array yields a struct element of the
                    // same tag (so `a[i].field` resolves); a non-struct base already
                    // carries no tag, so this leaves it `None`.
                }
                Token::Arrow | Token::Dot => {
                    self.advance();
                    let field = self.parse_identifier()?;
                    let tag = struct_tag.take().ok_or_else(|| {
                        Diagnostic::error(format!(
                            "member '{field}' on a non-struct-pointer base: {expression:?}"
                        ))
                    })?;
                    let is_function_pointer_field = self
                        .structs
                        .get(&tag)
                        .is_some_and(|layout| layout.function_pointer_fields.contains(&field));
                    let explicit_template_argument = self.try_explicit_member_template_argument();
                    if *self.peek() == Token::ParenOpen && !is_function_pointer_field {
                        // A non-virtual instance method is a direct call with
                        // the object pointer prepended as the implicit `this`.
                        // Virtual declarations are deliberately absent from the
                        // recovered method map and continue to defer below.
                        let mut arguments = Vec::new();
                        self.advance();
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
                        let direct_call = match explicit_template_argument {
                            Some(template_argument) => self.resolve_member_template_forwarder(
                                &tag,
                                &field,
                                template_argument,
                                arguments.len(),
                            ),
                            None => {
                                self.resolve_instance_member_call(&tag, &field, arguments.len())
                            }
                        }?;
                        if let Some(name) = direct_call {
                            arguments.insert(0, expression);
                            expression = Expression::Call { name, arguments };
                        } else if let Some(dispatch) = explicit_template_argument
                            .is_none()
                            .then(|| {
                                self.resolve_virtual_member_call(&tag, &field, arguments.len())
                            })
                            .transpose()?
                            .flatten()
                        {
                            expression = Expression::VirtualCall {
                                object: Box::new(expression),
                                vptr_offset: dispatch.vptr_offset,
                                slot_offset: dispatch.slot_offset,
                                return_type: dispatch.return_type,
                                variadic: dispatch.variadic,
                                arguments,
                            };
                        } else {
                            let kind = if explicit_template_argument.is_some() {
                                "member-template"
                            } else {
                                "member"
                            };
                            return Err(Diagnostic::error(format!(
                                "C++ {kind} call '{tag}::{field}' is inline or unavailable (roadmap)"
                            )));
                        }
                        struct_tag = None;
                        continue;
                    }
                    let layout = self.structs.get(&tag).ok_or_else(|| {
                        Diagnostic::error(format!("struct '{tag}' is not declared"))
                    })?;
                    let struct_size = layout.size;
                    let member = layout.fields.get(&field).ok_or_else(|| {
                        Diagnostic::error(format!("struct '{tag}' has no member '{field}'"))
                    })?;
                    let bit_field = member.bit_field;
                    let signed = member.member_type.is_signed();
                    let (offset, member_type, next_tag, array_element, array_bytes, array_stride) = (
                        member.offset,
                        member.member_type,
                        member.struct_tag.clone(),
                        member.array_element,
                        member.array_bytes,
                        member.array_stride,
                    );
                    // `a[i].field`: the index scales by the struct size — recorded so
                    // codegen can emit `a + i*size + offset`.
                    let index_stride =
                        matches!(expression, Expression::Index { .. }).then_some(struct_size);
                    // `(*p).field` is exactly `p->field`: the member's base is the pointer
                    // itself, so unwrap one dereference level here (the index_stride check
                    // above already saw the original shape). Without this the base would be
                    // `*p` and codegen would emit a spurious extra load.
                    let mut base_offset = 0u32;
                    let mut base_stride: Option<u32> = None;
                    expression = match expression {
                        Expression::Dereference { pointer } => *pointer,
                        // An EMBEDDED struct-value member folds into its base:
                        // `p->state.eof` is ONE access at offset(state)+offset(eof)
                        // — a struct VALUE member is storage, not a pointer, so no
                        // intermediate load exists.
                        Expression::Member {
                            base,
                            offset: outer_offset,
                            member_type: Type::Struct { .. },
                            index_stride: outer_stride,
                        } => {
                            base_offset = outer_offset;
                            base_stride = outer_stride;
                            *base
                        }
                        other => other,
                    };
                    let offset = offset + base_offset;
                    let index_stride = base_stride.or(index_stride);
                    if array_element.is_none() && array_stride.is_some() {
                        return Err(Diagnostic::error(format!(
                            "accessing pointer-to-array member '{field}' is not supported yet (roadmap)"
                        )));
                    }
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
                        let shift =
                            load_bits - (bit_offset as u32 - byte_start as u32 * 8) - width as u32;
                        let load = Expression::Member {
                            base: Box::new(expression),
                            offset: offset + u32::from(byte_start),
                            member_type: load_type,
                            index_stride,
                        };
                        let storage = load.clone();
                        let value = if shift > 0 {
                            Expression::Binary {
                                operator: mwcc_syntax_trees::BinaryOperator::ShiftRight,
                                left: Box::new(load),
                                right: Box::new(Expression::IntegerLiteral(shift as i64)),
                            }
                        } else {
                            load
                        };
                        let extracted = if width as u32 == load_bits {
                            value
                        } else {
                            let mask = (1i64 << width) - 1;
                            Expression::Binary {
                                operator: mwcc_syntax_trees::BinaryOperator::BitAnd,
                                left: Box::new(value),
                                right: Box::new(Expression::IntegerLiteral(mask)),
                            }
                        };
                        // A narrow unsigned bit-field undergoes integer promotion to
                        // `int`; a full-width unsigned field remains `unsigned int`.
                        // Keep that conversion explicit in the AST instead of erasing
                        // the bit-field provenance into an ordinary load/shift/mask.
                        expression = Expression::BitFieldRead {
                            promoted_type: if width < 32 {
                                Type::Int
                            } else {
                                Type::UnsignedInt
                            },
                            extracted: Box::new(extracted),
                            storage: Box::new(storage),
                            shift: shift as u8,
                            width,
                        };
                        struct_tag = None;
                        continue;
                    }
                    expression = match array_element {
                        // An array member decays to the address of its first element.
                        Some(element) => {
                            self.last_member_array_bytes = array_bytes;
                            Expression::MemberAddress {
                                base: Box::new(expression),
                                offset,
                                element,
                                index_stride: array_stride,
                            }
                        }
                        None => Expression::Member {
                            base: Box::new(expression),
                            offset,
                            member_type,
                            index_stride,
                        },
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
            return Ok(Expression::PostStep {
                target: Box::new(expression),
                operator,
            });
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
pub(crate) fn substitute_variables(
    expression: &Expression,
    map: &std::collections::HashMap<&str, &Expression>,
) -> Expression {
    match expression {
        Expression::Variable(name) => match map.get(name.as_str()) {
            Some(&replacement) => replacement.clone(),
            None => expression.clone(),
        },
        Expression::Binary {
            operator,
            left,
            right,
        } => Expression::Binary {
            operator: *operator,
            left: Box::new(substitute_variables(left, map)),
            right: Box::new(substitute_variables(right, map)),
        },
        Expression::Unary { operator, operand } => Expression::Unary {
            operator: *operator,
            operand: Box::new(substitute_variables(operand, map)),
        },
        Expression::Cast {
            target_type,
            operand,
        } => Expression::Cast {
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
        Expression::Member {
            base,
            offset,
            member_type,
            index_stride,
        } => Expression::Member {
            base: Box::new(substitute_variables(base, map)),
            offset: *offset,
            member_type: *member_type,
            index_stride: *index_stride,
        },
        Expression::MemberAddress {
            base,
            offset,
            element,
            index_stride,
        } => Expression::MemberAddress {
            base: Box::new(substitute_variables(base, map)),
            offset: *offset,
            element: *element,
            index_stride: *index_stride,
        },
        Expression::Conditional {
            condition,
            when_true,
            when_false,
            origin,
        } => Expression::Conditional {
            condition: Box::new(substitute_variables(condition, map)),
            when_true: Box::new(substitute_variables(when_true, map)),
            when_false: Box::new(substitute_variables(when_false, map)),
            origin: *origin,
        },
        Expression::Call { name, arguments } => Expression::Call {
            name: name.clone(),
            arguments: arguments
                .iter()
                .map(|argument| substitute_variables(argument, map))
                .collect(),
        },
        Expression::Assign { target, value } => Expression::Assign {
            target: Box::new(substitute_variables(target, map)),
            value: Box::new(substitute_variables(value, map)),
        },
        other => other.clone(),
    }
}

/// Whether substituting a single-return inline body preserves the caller's
/// argument evaluations. Variables and constants are stable even when the body
/// mentions them repeatedly. A side-effect-free read chain is safe only when its
/// parameter occurs once on an unconditional expression path.
fn inline_arguments_are_safe(
    parameters: &[String],
    body: &Expression,
    arguments: &[Expression],
) -> bool {
    parameters
        .iter()
        .zip(arguments)
        .all(|(parameter, argument)| {
            matches!(
                argument,
                Expression::Variable(_)
                    | Expression::IntegerLiteral(_)
                    | Expression::FloatLiteral(_)
            ) || (is_side_effect_free_read(argument)
                && unconditional_use_count(body, parameter) == Some(1))
        })
}

fn is_side_effect_free_read(expression: &Expression) -> bool {
    match expression {
        Expression::Variable(_)
        | Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_) => true,
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::Dereference { pointer: operand }
        | Expression::AddressOf { operand }
        | Expression::Member { base: operand, .. }
        | Expression::MemberAddress { base: operand, .. } => is_side_effect_free_read(operand),
        Expression::Index { base, index } => {
            is_side_effect_free_read(base) && is_side_effect_free_read(index)
        }
        _ => false,
    }
}

/// Count parameter uses while rejecting constructs that may skip or reorder the
/// evaluation relative to another effect. The accepted tree is deliberately
/// narrow: accessors, casts, and ordinary arithmetic over pure operands.
fn unconditional_use_count(expression: &Expression, name: &str) -> Option<usize> {
    match expression {
        Expression::Variable(variable) => Some(usize::from(variable == name)),
        Expression::IntegerLiteral(_)
        | Expression::FloatLiteral(_)
        | Expression::StringLiteral(_) => Some(0),
        Expression::Unary { operand, .. }
        | Expression::Cast { operand, .. }
        | Expression::Dereference { pointer: operand }
        | Expression::AddressOf { operand }
        | Expression::Member { base: operand, .. }
        | Expression::MemberAddress { base: operand, .. } => {
            unconditional_use_count(operand, name)
        }
        Expression::Index { base, index } => {
            Some(
                unconditional_use_count(base, name)?
                    + unconditional_use_count(index, name)?,
            )
        }
        Expression::Binary {
            operator,
            left,
            right,
        } if !matches!(operator, BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr) => Some(
            unconditional_use_count(left, name)? + unconditional_use_count(right, name)?,
        ),
        // Calls, assignments, short-circuit/conditional forms, and expression
        // kinds whose value can carry hidden storage are not parser-level inline
        // substitution candidates.
        _ => None,
    }
}
