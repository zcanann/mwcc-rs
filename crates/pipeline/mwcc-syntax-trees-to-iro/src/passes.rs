//! IRO passes: tree-to-tree rewrites MWCC's optimizer makes before code
//! generation. Each is a separate function over the typed IR.
//!
//! * [`fold`]: constant folding (literals, nested constant chains, typed
//!   pointer constants), `(x + y) - y`, `x * 0`.
//! * [`algebra`]: negation algebra and constant operands to the right of
//!   commutative operations.
//! * [`idioms`]: the 2.4.x sign-mask selects (`abs`, masked selects).
//! * [`selects`]: two-way selects become straight-line assignments with one
//!   conditional overwrite (or a truth value), as MWCC's IRO arranges them.
//! * [`stores`]: conversions a narrow store makes dead, literal truncation.

use mwcc_iro::{
    is_narrow, is_unsigned_narrow, width, BinaryOp, Expr, ExprKind, Function, Idiom, Stmt, Type, UnaryOp, VarId,
};

/// Run every pass in order.
pub fn run(function: &mut Function) {
    let enabled = |name: &str| std::env::var_os(format!("MWCC_IRO_NO_{name}")).is_none();
    if enabled("FOLD") {
        for_each_expression(&mut function.body, &mut |expression| fold(expression));
    }
    if enabled("ALGEBRA") {
        for_each_expression(&mut function.body, &mut |expression| algebra(expression));
    }
    if enabled("DISPLACEMENTS") {
        displacements(&mut function.body);
    }
    if enabled("IDIOMS") {
        let variables: Vec<Type> = function.variables.iter().map(|variable| variable.ty).collect();
        for_each_expression(&mut function.body, &mut |expression| idioms(expression, &variables));
    }
    if enabled("SELECTS") {
        selects(function);
    }
    if enabled("STORES") {
        stores(&mut function.body);
    }
    if enabled("NARROWING") {
        narrowing(function);
    }
}

/// Promotions a later narrowing discards: under a conversion to (or a store
/// of) a narrow type, low-bit operations need no extended operands, so a
/// promoted narrow value at least that wide is used as it is.
pub fn narrowing(function: &mut Function) {
    let variables: Vec<Type> = function.variables.iter().map(|variable| variable.ty).collect();
    narrowing_in(&mut function.body, function.return_type, &variables);
}

fn narrowing_in(body: &mut [Stmt], return_type: Type, variables: &[Type]) {
    fn strip(expression: &mut Expr, bytes: u32) {
        match &mut expression.kind {
            ExprKind::Convert(operand)
                if !is_narrow(expression.ty) && is_narrow(operand.ty) && width(operand.ty) >= bytes =>
            {
                *expression = (**operand).clone();
            }
            ExprKind::Binary(op, left, right)
                if matches!(
                    op,
                    BinaryOp::Add
                        | BinaryOp::Subtract
                        | BinaryOp::Multiply
                        | BinaryOp::BitAnd
                        | BinaryOp::BitOr
                        | BinaryOp::BitXor
                        | BinaryOp::ShiftLeft
                ) =>
            {
                strip(left, bytes);
                // A shift amount is not a low-bit operand.
                if *op != BinaryOp::ShiftLeft {
                    strip(right, bytes);
                }
            }
            ExprKind::Unary(UnaryOp::Negate | UnaryOp::BitNot, operand) => strip(operand, bytes),
            _ => {}
        }
    }
    fn visit(expression: &mut Expr) {
        children(expression, &mut |child| visit(child));
        if let ExprKind::Convert(operand) = &mut expression.kind {
            if is_narrow(expression.ty) {
                let bytes = width(expression.ty);
                strip(operand, bytes);
            }
        }
    }
    for statement in body.iter_mut() {
        // Values narrowed on the way out: stores, narrow variables and
        // results.
        let narrowed = match statement {
            Stmt::Store { ty, value, .. } => Some((*ty, value)),
            Stmt::Assign { variable, value } => Some((variables[*variable], value)),
            Stmt::SetReturn(value) | Stmt::Return(Some(value)) => Some((return_type, value)),
            Stmt::If { then_body, else_body, .. } => {
                narrowing_in(then_body, return_type, variables);
                narrowing_in(else_body, return_type, variables);
                None
            }
            Stmt::Loop { body, step, .. } => {
                narrowing_in(body, return_type, variables);
                narrowing_in(step, return_type, variables);
                None
            }
            _ => None,
        };
        if let Some((ty, value)) = narrowed {
            if is_narrow(ty) {
                strip(value, width(ty));
            }
        }
    }
    for_each_expression(body, &mut |expression| visit(expression));
}

/// `-O0`: only the front end's constant folding of literal operations.
pub fn run_unoptimized(function: &mut Function) {
    fn literals(expression: &mut Expr) {
        children(expression, &mut |child| literals(child));
        let folded = match &expression.kind {
            ExprKind::Convert(operand)
                if matches!(expression.ty, Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. }) =>
            {
                operand.as_int().map(|value| Expr::typed_int(value, expression.ty))
            }
            ExprKind::Binary(op, left, right) => match (left.as_int(), right.as_int()) {
                (Some(a), Some(b)) => fold_literals(*op, a, b).map(|value| Expr::typed_int(value, expression.ty)),
                _ => None,
            },
            _ => None,
        };
        if let Some(folded) = folded {
            *expression = folded;
        }
    }
    for_each_expression(&mut function.body, &mut |expression| literals(expression));
    stores(&mut function.body);
    displacements(&mut function.body);
    narrowing(function);
}

/// A constant addend of an access's base pointer joins its displacement:
/// `*(p - 8 + 2)` is `-24(p)`.
pub fn displacements(body: &mut [Stmt]) {
    fn absorb(base: &mut Box<Expr>, index: &Option<Box<Expr>>, offset: &mut i32) {
        if index.is_some() {
            return;
        }
        loop {
            let ExprKind::Binary(op @ (BinaryOp::Add | BinaryOp::Subtract), inner, addend) = &base.kind else { break };
            let (Some(value), true) = (addend.as_int(), pointer_like(inner.ty)) else { break };
            let value = if *op == BinaryOp::Subtract { -value } else { value };
            let Ok(total) = i16::try_from(i64::from(*offset) + value) else { break };
            *offset = i32::from(total);
            *base = inner.clone();
        }
    }
    fn visit(expression: &mut Expr) {
        children(expression, &mut |child| visit(child));
        if let ExprKind::Load { base, index, offset } = &mut expression.kind {
            absorb(base, index, offset);
        }
    }
    for statement in body.iter_mut() {
        match statement {
            Stmt::Store { place: mwcc_iro::Place::Memory { base, index, offset }, .. } => absorb(base, index, offset),
            Stmt::If { then_body, else_body, .. } => {
                displacements(then_body);
                displacements(else_body);
            }
            Stmt::Loop { body, step, .. } => {
                displacements(body);
                displacements(step);
            }
            _ => {}
        }
    }
    for_each_expression(body, &mut |expression| visit(expression));
}

/// Apply `rewrite` to every expression tree in `body` (statement roots).
pub fn for_each_expression(body: &mut [Stmt], rewrite: &mut dyn FnMut(&mut Expr)) {
    for statement in body {
        match statement {
            Stmt::Assign { value, .. } | Stmt::Eval(value) | Stmt::SetReturn(value) => rewrite(value),
            Stmt::Return(value) => {
                if let Some(value) = value {
                    rewrite(value);
                }
            }
            Stmt::Store { place, value, .. } => {
                if let mwcc_iro::Place::Memory { base, index, .. } = place {
                    rewrite(base);
                    if let Some(index) = index {
                        rewrite(index);
                    }
                }
                rewrite(value);
            }
            Stmt::If { condition, then_body, else_body } => {
                rewrite(condition);
                for_each_expression(then_body, rewrite);
                for_each_expression(else_body, rewrite);
            }
            Stmt::Loop { condition, body, step, .. } => {
                if let Some(condition) = condition {
                    rewrite(condition);
                }
                for_each_expression(body, rewrite);
                for_each_expression(step, rewrite);
            }
            Stmt::Break | Stmt::Continue => {}
        }
    }
}

fn children(expression: &mut Expr, rewrite: &mut dyn FnMut(&mut Expr)) {
    match &mut expression.kind {
        ExprKind::Int(_) | ExprKind::Var(_) | ExprKind::Global(_) | ExprKind::GlobalAddress(_) => {}
        ExprKind::Load { base, index, .. } => {
            rewrite(base);
            if let Some(index) = index {
                rewrite(index);
            }
        }
        ExprKind::Unary(_, operand) | ExprKind::Convert(operand) => rewrite(operand),
        ExprKind::Binary(_, left, right) => {
            rewrite(left);
            rewrite(right);
        }
        ExprKind::Select { condition, when_true, when_false } => {
            rewrite(condition);
            rewrite(when_true);
            rewrite(when_false);
        }
        ExprKind::Call { arguments, .. } => arguments.iter_mut().for_each(|argument| rewrite(argument)),
        ExprKind::Idiom(Idiom::Absolute(value)) => rewrite(value),
        ExprKind::Idiom(Idiom::Masked { tested, value, .. }) => {
            rewrite(tested);
            rewrite(value);
        }
    }
}

// ---------------------------------------------------------------- folding

/// Constant folding, bottom-up.
pub fn fold(expression: &mut Expr) {
    children(expression, &mut |child| fold(child));
    loop {
        let Some(folded) = fold_once(expression) else { break };
        *expression = folded;
    }
}

fn fold_once(expression: &Expr) -> Option<Expr> {
    match &expression.kind {
        // Integer and pointer casts of a literal are the literal, typed.
        ExprKind::Convert(operand)
            if matches!(expression.ty, Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. })
                && operand.as_int().is_some() =>
        {
            Some(Expr::typed_int(operand.as_int()?, expression.ty))
        }
        // A pointer-to-pointer (or word-to-word) conversion changes no bits.
        ExprKind::Convert(operand)
            if pointer_like(expression.ty) && (pointer_like(operand.ty) || matches!(operand.ty, Type::Int | Type::UnsignedInt)) =>
        {
            let mut value = (**operand).clone();
            value.ty = expression.ty;
            Some(value)
        }
        ExprKind::Binary(op, left, right) => {
            // `(x & 1) == 1` and `(x & 1) != 0` are `x & 1`.
            if let ExprKind::Binary(BinaryOp::BitAnd, _, mask) = &left.kind {
                if mask.as_int() == Some(1)
                    && (*op == BinaryOp::Equal && right.as_int() == Some(1)
                        || *op == BinaryOp::NotEqual && right.as_int() == Some(0))
                {
                    let mut value = (**left).clone();
                    value.ty = Type::Int;
                    return Some(value);
                }
            }
            if let (Some(a), Some(b)) = (left.as_int(), right.as_int()) {
                return Some(Expr::typed_int(fold_literals(*op, a, b)?, expression.ty));
            }
            // `x << 0`, `x >> 0`, `x + 0`, `x - 0`, `x | 0`, `x ^ 0` are `x`.
            if right.as_int() == Some(0)
                && matches!(
                    op,
                    BinaryOp::ShiftLeft
                        | BinaryOp::ShiftRight
                        | BinaryOp::Add
                        | BinaryOp::Subtract
                        | BinaryOp::BitOr
                        | BinaryOp::BitXor
                )
            {
                let mut value = (**left).clone();
                value.ty = expression.ty;
                return Some(value);
            }
            // `x * 0` is 0 when `x` has no effects.
            if *op == BinaryOp::Multiply && right.as_int() == Some(0) && speculable(left) {
                return Some(Expr::typed_int(0, expression.ty));
            }
            if let Some(chain) = fold_chain(*op, left, right, expression.ty) {
                return Some(chain);
            }
            cancel(*op, left, right).cloned()
        }
        _ => None,
    }
}

fn pointer_like(ty: Type) -> bool {
    matches!(ty, Type::Pointer(_) | Type::StructPointer { .. })
}

/// 32-bit arithmetic on two literals.
pub fn fold_literals(op: BinaryOp, left: i64, right: i64) -> Option<i64> {
    let (a, b) = (left as i32, right as i32);
    let value = match op {
        BinaryOp::Add => a.wrapping_add(b),
        BinaryOp::Subtract => a.wrapping_sub(b),
        BinaryOp::Multiply => a.wrapping_mul(b),
        BinaryOp::BitAnd => a & b,
        BinaryOp::BitOr => a | b,
        BinaryOp::BitXor => a ^ b,
        BinaryOp::ShiftLeft if (0..32).contains(&b) => a.wrapping_shl(b as u32),
        BinaryOp::Less => i32::from(a < b),
        BinaryOp::Greater => i32::from(a > b),
        BinaryOp::LessEqual => i32::from(a <= b),
        BinaryOp::GreaterEqual => i32::from(a >= b),
        BinaryOp::Equal => i32::from(a == b),
        BinaryOp::NotEqual => i32::from(a != b),
        BinaryOp::LogicalAnd => i32::from(a != 0 && b != 0),
        BinaryOp::LogicalOr => i32::from(a != 0 || b != 0),
        _ => return None,
    };
    Some(i64::from(value))
}

/// A constant operation applied to another constant operation of the same
/// kind: `(x >> 2) >> 3` -> `x >> 5`, `(x + 3) + 5` -> `x + 8`,
/// `x + 10 - 3` -> `x + 7`, `(x | 5) | 2` -> `x | 7`.
fn fold_chain(op: BinaryOp, left: &Expr, right: &Expr, ty: Type) -> Option<Expr> {
    let outer = right.as_int()?;
    let ExprKind::Binary(inner_op, inner_left, inner_right) = &left.kind else { return None };
    let inner = inner_right.as_int()?;
    use BinaryOp::*;
    let (op, value) = match (*inner_op, op) {
        (ShiftRight, ShiftRight) | (ShiftLeft, ShiftLeft) if inner + outer < 32 => (op, inner + outer),
        (Add, Add) => (Add, inner + outer),
        (Add, Subtract) => (Add, inner - outer),
        (Subtract, Add) => (Add, outer - inner),
        (Subtract, Subtract) => (Subtract, inner + outer),
        (BitOr, BitOr) => (BitOr, inner | outer),
        (BitXor, BitXor) => (BitXor, inner ^ outer),
        (BitAnd, BitAnd) => (BitAnd, inner & outer),
        (Multiply, Multiply) => (Multiply, inner * outer),
        _ => return None,
    };
    Some(Expr::binary(op, (**inner_left).clone(), Expr::int(value), ty))
}

/// `(x + y) - y` / `(x - y) + y` -> `x` (matching leaf operands only).
fn cancel<'a>(op: BinaryOp, left: &'a Expr, right: &Expr) -> Option<&'a Expr> {
    let ExprKind::Binary(inner, x, y) = &left.kind else { return None };
    let inverse = matches!((inner, op), (BinaryOp::Add, BinaryOp::Subtract) | (BinaryOp::Subtract, BinaryOp::Add));
    let same = match (&y.kind, &right.kind) {
        (ExprKind::Var(a), ExprKind::Var(b)) => a == b,
        (ExprKind::Int(a), ExprKind::Int(b)) => a == b,
        _ => false,
    };
    (inverse && same).then_some(x.as_ref())
}

/// A value MWCC evaluates unconditionally: no calls, and nothing that can
/// trap (pointer loads, division).
pub fn speculable(expression: &Expr) -> bool {
    match &expression.kind {
        ExprKind::Int(_) | ExprKind::Var(_) | ExprKind::Global(_) | ExprKind::GlobalAddress(_) => true,
        ExprKind::Binary(op, left, right) => {
            !matches!(op, BinaryOp::Divide | BinaryOp::Modulo | BinaryOp::LogicalAnd | BinaryOp::LogicalOr)
                && speculable(left)
                && speculable(right)
        }
        ExprKind::Unary(_, operand) | ExprKind::Convert(operand) => speculable(operand),
        _ => false,
    }
}

// ---------------------------------------------------------------- algebra

/// Negation algebra and commutative-constant canonicalization, bottom-up.
pub fn algebra(expression: &mut Expr) {
    children(expression, &mut |child| algebra(child));
    if let Some(rewritten) = negation(expression) {
        *expression = rewritten;
        algebra(expression);
        return;
    }
    // A constant goes on the right of a commutative operation (an immediate).
    if let ExprKind::Binary(op, left, right) = &mut expression.kind {
        if op.is_commutative() && left.as_int().is_some() && right.as_int().is_none() {
            std::mem::swap(left, right);
        }
    }
}

/// `a - -b` = `a + b`, `a + -b` = `a - b`, `-a + b` = `b - a`,
/// `-a - b` = `-(a + b)`.
fn negation(expression: &Expr) -> Option<Expr> {
    let ExprKind::Binary(op, left, right) = &expression.kind else { return None };
    let negated = |e: &Expr| match &e.kind {
        ExprKind::Unary(UnaryOp::Negate, operand) => Some((**operand).clone()),
        _ => None,
    };
    let ty = expression.ty;
    match op {
        BinaryOp::Subtract => {
            if let Some(b) = negated(right) {
                return Some(Expr::binary(BinaryOp::Add, (**left).clone(), b, ty));
            }
            let a = negated(left)?;
            Some(Expr::unary(UnaryOp::Negate, Expr::binary(BinaryOp::Add, a, (**right).clone(), ty), ty))
        }
        BinaryOp::Add => {
            if let Some(b) = negated(right) {
                return Some(Expr::binary(BinaryOp::Subtract, (**left).clone(), b, ty));
            }
            let a = negated(left)?;
            Some(Expr::binary(BinaryOp::Subtract, (**right).clone(), a, ty))
        }
        _ => None,
    }
}

// ---------------------------------------------------------------- idioms

/// Rewrite recognized sign-mask selects into [`Idiom`] nodes, bottom-up.
pub fn idioms(expression: &mut Expr, variables: &[Type]) {
    children(expression, &mut |child| idioms(child, variables));
    if let ExprKind::Select { condition, when_true, when_false } = &expression.kind {
        if let Some(idiom) = sign_idiom(condition, when_true, when_false, variables) {
            let ty = match &idiom {
                Idiom::Absolute(_) => Type::Int,
                Idiom::Masked { value, .. } => mwcc_iro::promote(value.ty),
            };
            *expression = Expr { kind: ExprKind::Idiom(idiom), ty };
        }
    }
}

/// The idiom for `condition ? when_true : when_false`, when the tested value
/// is a signed int (or, for equality masks, any word).
pub fn sign_idiom(condition: &Expr, when_true: &Expr, when_false: &Expr, variables: &[Type]) -> Option<Idiom> {
    let idiom = recognize(condition, when_true, when_false)?;
    let tested = match &idiom {
        Idiom::Absolute(tested) | Idiom::Masked { tested, .. } => tested,
    };
    let equality = matches!(idiom, Idiom::Masked { relation: BinaryOp::Equal | BinaryOp::NotEqual, .. });
    let ty = variables[tested.as_var()?];
    (ty == Type::Int || equality && matches!(ty, Type::UnsignedInt | Type::Pointer(_))).then_some(idiom)
}

fn recognize(condition: &Expr, when_true: &Expr, when_false: &Expr) -> Option<Idiom> {
    let is_var = |e: &Expr| e.as_var().is_some();
    // A bare truth value tests `!= 0`.
    if is_var(condition) {
        return match (is_var(when_true), when_false.as_int()) {
            (true, Some(0)) => Some(Idiom::Masked {
                relation: BinaryOp::NotEqual,
                tested: Box::new(condition.clone()),
                value: Box::new(when_true.clone()),
                keep_when_true: true,
            }),
            _ => None,
        };
    }
    let ExprKind::Binary(op, left, right) = &condition.kind else { return None };
    let (relation, tested) = match (is_var(left), left.as_int(), is_var(right), right.as_int()) {
        (true, _, _, Some(0)) => (*op, left),
        (_, Some(0), true, _) => (op.mirror(), right),
        _ => return None,
    };
    if !relation.is_comparison() {
        return None;
    }
    let tested_id = tested.as_var()?;
    let equality = matches!(relation, BinaryOp::Equal | BinaryOp::NotEqual);
    let negation_of = |e: &Expr| matches!(&e.kind, ExprKind::Unary(UnaryOp::Negate, operand) if operand.as_var() == Some(tested_id));
    let same = |e: &Expr| e.as_var() == Some(tested_id);
    let negative_side = matches!(relation, BinaryOp::Less | BinaryOp::LessEqual);
    if !equality
        && (negative_side && negation_of(when_true) && same(when_false)
            || !negative_side && same(when_true) && negation_of(when_false))
    {
        return Some(Idiom::Absolute(tested.clone()));
    }
    if is_var(when_true) && when_false.as_int() == Some(0) {
        return Some(Idiom::Masked {
            relation,
            tested: tested.clone(),
            value: Box::new(when_true.clone()),
            keep_when_true: true,
        });
    }
    if when_true.as_int() == Some(0)
        && is_var(when_false)
        && matches!(relation, BinaryOp::Less | BinaryOp::Greater | BinaryOp::Equal | BinaryOp::NotEqual)
    {
        return Some(Idiom::Masked {
            relation,
            tested: tested.clone(),
            value: Box::new(when_false.clone()),
            keep_when_true: false,
        });
    }
    None
}

// ---------------------------------------------------------------- selects

/// Where a select's value goes.
#[derive(Clone, Copy)]
enum Destination {
    Variable(VarId),
    Return,
}

impl Destination {
    fn assign(self, value: Expr) -> Stmt {
        match self {
            Destination::Variable(variable) => Stmt::Assign { variable, value },
            Destination::Return => Stmt::SetReturn(value),
        }
    }
}

/// Two-way selects: `if (c) x = A; else x = B;`, `if (c) return A; else
/// return B;`, a last guard with the final return, and `return c ? A : B`.
/// MWCC assigns one arm unconditionally and overwrites it on one path (the
/// else arm first unless only the then arm is a constant); a 1/0 select is
/// the condition's truth value.
pub fn selects(function: &mut Function) {
    let mut body = std::mem::take(&mut function.body);
    rewrite_selects(function, &mut body, true);
    function.body = body;
}

fn rewrite_selects(function: &mut Function, body: &mut Vec<Stmt>, top_level: bool) {
    for statement in body.iter_mut() {
        match statement {
            Stmt::If { then_body, else_body, .. } => {
                rewrite_selects(function, then_body, false);
                rewrite_selects(function, else_body, false);
            }
            Stmt::Loop { body, step, .. } => {
                rewrite_selects(function, body, false);
                rewrite_selects(function, step, false);
            }
            _ => {}
        }
    }
    let mut output = Vec::with_capacity(body.len());
    let statements = std::mem::take(body);
    let count = statements.len();
    let mut iter = statements.into_iter().enumerate().peekable();
    while let Some((index, statement)) = iter.next() {
        // The last guard and the final return value.
        if top_level && index + 2 == count {
            if let Stmt::If { condition, then_body, else_body } = &statement {
                if let ([Stmt::Return(Some(value))], [], Some((_, Stmt::SetReturn(final_value)))) =
                    (then_body.as_slice(), else_body.as_slice(), iter.peek())
                {
                    if let Some(rewritten) = select(function, condition, value, final_value, Destination::Return) {
                        output.extend(rewritten);
                        iter.next();
                        continue;
                    }
                }
            }
        }
        match statement {
            Stmt::If { condition, then_body, else_body } => {
                match (then_body.as_slice(), else_body.as_slice()) {
                    (
                        [Stmt::Assign { variable, value }],
                        [Stmt::Assign { variable: other, value: other_value }],
                    ) if variable == other => {
                        let variable = *variable;
                        // Reading the variable in the condition or an arm
                        // selects into a temporary copied back afterwards.
                        let reads = condition.mentions(variable)
                            || value.mentions(variable)
                            || other_value.mentions(variable);
                        let destination = if reads {
                            Destination::Variable(function.add_temporary(function.variables[variable].ty))
                        } else {
                            Destination::Variable(variable)
                        };
                        if let Some(rewritten) = select(function, &condition, value, other_value, destination) {
                            output.extend(rewritten);
                            if let (true, Destination::Variable(temporary)) = (reads, destination) {
                                let ty = function.variables[temporary].ty;
                                output.push(Stmt::Assign {
                                    variable,
                                    value: Expr { kind: ExprKind::Var(temporary), ty },
                                });
                            }
                            continue;
                        }
                    }
                    ([Stmt::Return(Some(value))], [Stmt::Return(Some(other_value))]) => {
                        if let Some(rewritten) = select(function, &condition, value, other_value, Destination::Return) {
                            output.extend(rewritten);
                            output.push(Stmt::Return(None));
                            continue;
                        }
                    }
                    _ => {}
                }
                output.push(Stmt::If { condition, then_body, else_body });
            }
            // `return c ? A : B` as the final value.
            Stmt::SetReturn(Expr { kind: ExprKind::Select { condition, when_true, when_false }, .. })
                if top_level && index + 1 == count =>
            {
                match select(function, &condition, &when_true, &when_false, Destination::Return) {
                    Some(rewritten) => output.extend(rewritten),
                    None => {
                        output.push(Stmt::If {
                            condition: *condition,
                            then_body: vec![Stmt::Return(Some(*when_true))],
                            else_body: Vec::new(),
                        });
                        output.push(Stmt::SetReturn(*when_false));
                    }
                }
            }
            other => output.push(other),
        }
    }
    *body = output;
}

/// Straight-line statements for `destination = condition ? when_true :
/// when_false`, or `None` when MWCC keeps the branches.
fn select(
    function: &Function,
    condition: &Expr,
    when_true: &Expr,
    when_false: &Expr,
    destination: Destination,
) -> Option<Vec<Stmt>> {
    // `!c ? A : B` is `c ? B : A`.
    if let ExprKind::Unary(UnaryOp::LogicalNot, operand) = &condition.kind {
        return select(function, operand, when_false, when_true, destination);
    }
    let variables: Vec<Type> = function.variables.iter().map(|variable| variable.ty).collect();
    if let Some(idiom) = sign_idiom(condition, when_true, when_false, &variables) {
        let ty = match &idiom {
            Idiom::Absolute(_) => Type::Int,
            Idiom::Masked { value, .. } => mwcc_iro::promote(value.ty),
        };
        return Some(vec![destination.assign(Expr { kind: ExprKind::Idiom(idiom), ty })]);
    }
    let simple = simple_condition(condition);
    if simple {
        match (when_true.as_int(), when_false.as_int()) {
            (Some(1), Some(0)) => {
                let truth = match &condition.kind {
                    ExprKind::Binary(op, ..) if op.is_comparison() => condition.clone(),
                    _ => Expr::binary(BinaryOp::NotEqual, condition.clone(), Expr::int(0), Type::Int),
                };
                return Some(vec![destination.assign(truth)]);
            }
            (Some(0), Some(1)) => {
                return Some(vec![destination.assign(Expr::unary(UnaryOp::LogicalNot, condition.clone(), Type::Int))]);
            }
            _ => {}
        }
    }
    if !simple || !speculable(when_true) || !speculable(when_false) {
        return None;
    }
    let constant = |e: &Expr| e.as_int().is_some();
    let then_first = constant(when_true) && !constant(when_false);
    let (first, second, second_condition) = if then_first {
        (when_true, when_false, Expr::unary(UnaryOp::LogicalNot, condition.clone(), Type::Int))
    } else {
        (when_false, when_true, condition.clone())
    };
    Some(vec![
        destination.assign(first.clone()),
        Stmt::If { condition: second_condition, then_body: vec![destination.assign(second.clone())], else_body: Vec::new() },
    ])
}

/// A branch condition MWCC turns into a single compare-and-branch.
fn simple_condition(condition: &Expr) -> bool {
    match &condition.kind {
        ExprKind::Unary(UnaryOp::LogicalNot, operand) => simple_condition(operand),
        ExprKind::Binary(BinaryOp::LogicalAnd | BinaryOp::LogicalOr, ..) => false,
        _ => true,
    }
}

// ---------------------------------------------------------------- stores

/// A store keeps only its low bytes: integer conversions at least that wide
/// are dead, and a literal stored narrow is its sign-extended low part.
pub fn stores(body: &mut [Stmt]) {
    for statement in body {
        match statement {
            Stmt::Store { ty, value, .. } => {
                let stored = width(*ty);
                while let ExprKind::Convert(operand) = &value.kind {
                    if !(mwcc_iro::is_general_word(value.ty) && !matches!(value.ty, Type::Pointer(_) | Type::StructPointer { .. }))
                        || width(value.ty) < stored
                    {
                        break;
                    }
                    *value = (**operand).clone();
                }
                if let (Some(literal), true) = (value.as_int(), is_narrow(*ty)) {
                    let shift = 64 - 8 * stored;
                    *value = Expr::int((literal << shift) >> shift);
                }
            }
            Stmt::If { then_body, else_body, .. } => {
                stores(then_body);
                stores(else_body);
            }
            Stmt::Loop { body, step, .. } => {
                stores(body);
                stores(step);
            }
            _ => {}
        }
    }
}

/// Whether `value` already fits the narrow type `ty` (a 0/1 truth value in
/// an unsigned type, or an in-range literal): no conversion is needed.
pub fn fits_unconverted(value: &Expr, ty: Type) -> bool {
    if !is_narrow(ty) {
        return false;
    }
    let unsigned = is_unsigned_narrow(ty);
    match &value.kind {
        ExprKind::Binary(op, ..) => unsigned && op.is_comparison(),
        ExprKind::Unary(UnaryOp::LogicalNot, _) => unsigned,
        ExprKind::Int(value) => match ty {
            Type::Char => (-128..=127).contains(value),
            Type::UnsignedChar => (0..=255).contains(value),
            Type::Short => (-32768..=32767).contains(value),
            Type::UnsignedShort => (0..=65535).contains(value),
            _ => false,
        },
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_iro::{Pointee, Variable, VariableKind};

    fn var(id: VarId) -> Expr {
        Expr { kind: ExprKind::Var(id), ty: Type::Int }
    }

    fn function(body: Vec<Stmt>) -> Function {
        Function {
            name: "f".into(),
            return_type: Type::Int,
            variables: (0..2)
                .map(|i| Variable { name: format!("p{i}"), ty: Type::Int, kind: VariableKind::Parameter })
                .collect(),
            parameter_count: 2,
            body,
        }
    }

    #[test]
    fn folds_scaled_pointer_constants_and_chains() {
        let pointer = Type::Pointer(Pointee::Int);
        let mut e = Expr::binary(BinaryOp::Add, Expr::typed_int(0x1000, pointer), Expr::int(12), pointer);
        fold(&mut e);
        assert_eq!(e.as_int(), Some(0x100c));
        assert_eq!(e.ty, pointer);
        let mut chain = Expr::binary(
            BinaryOp::Subtract,
            Expr::binary(BinaryOp::Add, var(0), Expr::int(10), Type::Int),
            Expr::int(3),
            Type::Int,
        );
        fold(&mut chain);
        assert!(matches!(&chain.kind, ExprKind::Binary(BinaryOp::Add, _, right) if right.as_int() == Some(7)));
    }

    #[test]
    fn negation_algebra() {
        let mut e = Expr::binary(BinaryOp::Subtract, var(0), Expr::unary(UnaryOp::Negate, var(1), Type::Int), Type::Int);
        algebra(&mut e);
        assert!(matches!(&e.kind, ExprKind::Binary(BinaryOp::Add, ..)));
    }

    #[test]
    fn select_hoists_else_arm_unless_only_then_is_constant() {
        let condition = Expr::binary(BinaryOp::Less, var(0), var(1), Type::Int);
        let mut f = function(vec![Stmt::If {
            condition: condition.clone(),
            then_body: vec![Stmt::Return(Some(Expr::int(3)))],
            else_body: vec![],
        }, Stmt::SetReturn(Expr::binary(BinaryOp::Add, var(0), var(1), Type::Int))]);
        selects(&mut f);
        // Then arm constant, else arm computed: the constant goes first and
        // the else arm is the conditional overwrite.
        assert!(matches!(&f.body[0], Stmt::SetReturn(value) if value.as_int() == Some(3)));
        assert!(matches!(&f.body[1], Stmt::If { condition, .. } if matches!(condition.kind, ExprKind::Unary(UnaryOp::LogicalNot, _))));
    }

    #[test]
    fn select_into_temporary_when_condition_reads_the_variable() {
        let mut f = function(vec![Stmt::If {
            condition: Expr::binary(BinaryOp::Less, var(0), var(1), Type::Int),
            then_body: vec![Stmt::Assign { variable: 0, value: Expr::binary(BinaryOp::Add, var(0), Expr::int(1), Type::Int) }],
            else_body: vec![Stmt::Assign { variable: 0, value: var(1) }],
        }]);
        selects(&mut f);
        // v0 is only written by the final copy from the temporary.
        let writes: Vec<VarId> = f.body.iter().filter_map(|s| match s { Stmt::Assign { variable, .. } => Some(*variable), _ => None }).collect();
        assert_eq!(writes.last(), Some(&0));
        assert!(writes[..writes.len() - 1].iter().all(|&v| v != 0));
    }

    #[test]
    fn abs_becomes_an_idiom() {
        let x = var(0);
        let mut e = Expr {
            kind: ExprKind::Select {
                condition: Box::new(Expr::binary(BinaryOp::Less, x.clone(), Expr::int(0), Type::Int)),
                when_true: Box::new(Expr::unary(UnaryOp::Negate, x.clone(), Type::Int)),
                when_false: Box::new(x),
            },
            ty: Type::Int,
        };
        idioms(&mut e, &[Type::Int, Type::Int]);
        assert!(matches!(e.kind, ExprKind::Idiom(Idiom::Absolute(_))));
    }
}
