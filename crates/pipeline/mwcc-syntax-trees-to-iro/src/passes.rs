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

use std::collections::HashMap;

use mwcc_iro::{Place, Variable, VariableKind,
    is_narrow, is_unsigned_narrow, width, BinaryOp, Expr, ExprKind, Function, Idiom, Stmt, Type, UnaryOp, VarId,
};

/// Scalar replacement: a frame object read and written only at constant
/// offsets becomes one register variable per field. With
/// `keeps_struct_stores` (GC/3.x, Wii) a struct still stores each field to
/// its frame slot; reads use the register.
pub fn scalarize(function: &mut Function, keeps_struct_stores: bool) {
    let candidates: Vec<VarId> = (0..function.variables.len())
        .filter(|&id| function.variables[id].frame.is_some() && function.variables[id].kind == VariableKind::Local)
        .collect();
    for id in candidates {
        let mut fields: Vec<(i32, Type)> = Vec::new();
        let mut escapes = false;
        scan_fields(&mut function.body, id, &mut fields, &mut escapes);
        if escapes || fields.is_empty() {
            continue;
        }
        fields.sort_by_key(|&(offset, _)| offset);
        fields.dedup();
        let consistent = fields.windows(2).all(|pair| {
            let ((a, ta), (b, _)) = (pair[0], pair[1]);
            a != b && a + mwcc_iro::width(ta) as i32 <= b
        });
        if !consistent || fields.iter().any(|&(_, ty)| !mwcc_iro::is_value_type(ty)) {
            continue;
        }
        let name = function.variables[id].name.clone();
        let mut map = HashMap::new();
        for &(offset, ty) in &fields {
            map.insert(offset, function.variables.len());
            function.variables.push(Variable {
                name: format!("{name}.{offset}"),
                ty,
                kind: VariableKind::Local,
                frame: None,
            });
        }
        let keep = keeps_struct_stores && matches!(function.variables[id].ty, Type::Struct { .. });
        let body = std::mem::take(&mut function.body);
        function.body = replace_fields(body, id, &map, keep);
    }
}

fn is_address_of(e: &Expr, id: VarId) -> bool {
    matches!(e.kind, ExprKind::LocalAddress(x) if x == id)
}

fn scan_fields(body: &mut [Stmt], id: VarId, fields: &mut Vec<(i32, Type)>, escapes: &mut bool) {
    fn expression(e: &mut Expr, id: VarId, fields: &mut Vec<(i32, Type)>, escapes: &mut bool) {
        match &e.kind {
            ExprKind::Load { base, index: None, offset } if is_address_of(base, id) => fields.push((*offset, e.ty)),
            ExprKind::LocalAddress(x) if *x == id => *escapes = true,
            _ => children(e, &mut |child| expression(child, id, fields, escapes)),
        }
    }
    for statement in body {
        match statement {
            Stmt::Store { place: Place::Memory { base, index: None, offset }, ty, value } if is_address_of(base, id) => {
                fields.push((*offset, *ty));
                expression(value, id, fields, escapes);
            }
            Stmt::If { condition, then_body, else_body } => {
                expression(condition, id, fields, escapes);
                scan_fields(then_body, id, fields, escapes);
                scan_fields(else_body, id, fields, escapes);
            }
            Stmt::Loop { condition, body, step, .. } => {
                if let Some(condition) = condition {
                    expression(condition, id, fields, escapes);
                }
                scan_fields(body, id, fields, escapes);
                scan_fields(step, id, fields, escapes);
            }
            Stmt::Switch { value, arms, .. } => {
                expression(value, id, fields, escapes);
                for arm in arms {
                    scan_fields(arm, id, fields, escapes);
                }
            }
            other => for_each_expression(std::slice::from_mut(other), &mut |e| expression(e, id, fields, escapes)),
        }
    }
}

fn replace_fields(body: Vec<Stmt>, id: VarId, map: &HashMap<i32, VarId>, keep: bool) -> Vec<Stmt> {
    fn expression(e: &mut Expr, id: VarId, map: &HashMap<i32, VarId>) {
        if let ExprKind::Load { base, index: None, offset } = &e.kind {
            if is_address_of(base, id) {
                e.kind = ExprKind::Var(map[offset]);
                return;
            }
        }
        children(e, &mut |child| expression(child, id, map));
    }
    let mut out = Vec::with_capacity(body.len());
    for mut statement in body {
        match statement {
            Stmt::Store { place: Place::Memory { base, index: None, offset }, ty, mut value } if is_address_of(&base, id) => {
                expression(&mut value, id, map);
                let variable = map[&offset];
                out.push(Stmt::Assign { variable, value });
                if keep {
                    out.push(Stmt::Store {
                        place: Place::Memory { base, index: None, offset },
                        ty,
                        value: Expr { kind: ExprKind::Var(variable), ty },
                    });
                }
            }
            Stmt::If { mut condition, then_body, else_body } => {
                expression(&mut condition, id, map);
                out.push(Stmt::If {
                    condition,
                    then_body: replace_fields(then_body, id, map, keep),
                    else_body: replace_fields(else_body, id, map, keep),
                });
            }
            Stmt::Loop { test_first, mut condition, body, step } => {
                if let Some(condition) = &mut condition {
                    expression(condition, id, map);
                }
                out.push(Stmt::Loop {
                    test_first,
                    condition,
                    body: replace_fields(body, id, map, keep),
                    step: replace_fields(step, id, map, keep),
                });
            }
            Stmt::Switch { mut value, cases, arms, default } => {
                expression(&mut value, id, map);
                let arms = arms.into_iter().map(|arm| replace_fields(arm, id, map, keep)).collect();
                out.push(Stmt::Switch { value, cases, arms, default });
            }
            _ => {
                for_each_expression(std::slice::from_mut(&mut statement), &mut |e| expression(e, id, map));
                out.push(statement);
            }
        }
    }
    out
}

/// Run every pass in order.
pub fn run(function: &mut Function) {
    let enabled = |name: &str| std::env::var_os(format!("MWCC_IRO_NO_{name}")).is_none();
    if enabled("FOLD") {
        for_each_expression(&mut function.body, &mut |expression| fold(expression));
    }
    if enabled("FORWARD") {
        forward_offsets(function);
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
            Stmt::Switch { arms, .. } => {
                for arm in arms {
                    narrowing_in(arm, return_type, variables);
                }
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
    displacements_with(&mut function.body, false);
    narrowing(function);
}

/// A constant addend of an access's base pointer joins its displacement:
/// `*(p - 8 + 2)` is `-24(p)`.
pub fn displacements(body: &mut [Stmt]) {
    displacements_with(body, std::env::var_os("MWCC_IRO_NO_INDEX_CONSTANT").is_none());
}

/// Displacement folding; `distribute` also moves a scaled index's constant
/// addend into the displacement (optimized builds only).
pub fn displacements_with(body: &mut [Stmt], distribute: bool) {
    fn absorb(base: &mut Box<Expr>, index: &mut Option<Box<Expr>>, offset: &mut i32, distribute: bool) {
        // `p[i + k]`: the constant part of the index joins the displacement
        // (`add` then `lwz k*size`).
        if let (Some(ix), true) = (index.as_deref(), distribute) {
            if let Some((variable, constant)) = constant_index_part(ix) {
                if let Ok(total) = i16::try_from(i64::from(*offset) + constant) {
                    let ty = base.ty;
                    *base = Box::new(Expr::binary(BinaryOp::Add, (**base).clone(), variable, ty));
                    *index = None;
                    *offset = i32::from(total);
                }
            }
        }
        if index.is_some() {
            return;
        }
        loop {
            if let (ExprKind::Binary(BinaryOp::Add, pointer, ix), true) = (&base.kind, distribute) {
                if pointer_like(pointer.ty) && !pointer_like(ix.ty) {
                    if let Some((variable, constant)) = constant_index_part(ix) {
                        if let Ok(total) = i16::try_from(i64::from(*offset) + constant) {
                            let ty = base.ty;
                            *base = Box::new(Expr::binary(BinaryOp::Add, (**pointer).clone(), variable, ty));
                            *offset = i32::from(total);
                            continue;
                        }
                    }
                }
            }
            let ExprKind::Binary(op @ (BinaryOp::Add | BinaryOp::Subtract), inner, addend) = &base.kind else { break };
            let (Some(value), true) = (addend.as_int(), pointer_like(inner.ty)) else { break };
            let value = if *op == BinaryOp::Subtract { -value } else { value };
            let Ok(total) = i16::try_from(i64::from(*offset) + value) else { break };
            *offset = i32::from(total);
            *base = inner.clone();
        }
    }
    fn visit(expression: &mut Expr, distribute: bool) {
        children(expression, &mut |child| visit(child, distribute));
        if let ExprKind::Load { base, index, offset } = &mut expression.kind {
            absorb(base, index, offset, distribute);
        }
    }
    for statement in body.iter_mut() {
        match statement {
            Stmt::Store { place: mwcc_iro::Place::Memory { base, index, offset }, .. } => {
                absorb(base, index, offset, distribute)
            }
            Stmt::If { then_body, else_body, .. } => {
                displacements_with(then_body, distribute);
                displacements_with(else_body, distribute);
            }
            Stmt::Loop { body, step, .. } => {
                displacements_with(body, distribute);
                displacements_with(step, distribute);
            }
            Stmt::Switch { arms, .. } => {
                for arm in arms {
                    displacements_with(arm, distribute);
                }
            }
            _ => {}
        }
    }
    for_each_expression(body, &mut |expression| visit(expression, distribute));
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
            Stmt::Switch { value, arms, .. } => {
                rewrite(value);
                for arm in arms {
                    for_each_expression(arm, rewrite);
                }
            }
            Stmt::Break | Stmt::Continue => {}
        }
    }
}

fn children(expression: &mut Expr, rewrite: &mut dyn FnMut(&mut Expr)) {
    match &mut expression.kind {
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Var(_)
        | ExprKind::Global(_)
        | ExprKind::GlobalAddress(_)
        | ExprKind::LocalAddress(_)
        | ExprKind::StringAddress(_) => {}
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
        ExprKind::Idiom(Idiom::Insert { base, value, .. }) => {
            rewrite(base);
            rewrite(value);
        }
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
    if let Some(folded) = fold_float(expression) {
        return Some(folded);
    }
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

/// Floating constants: conversions of literals and arithmetic on two.
fn fold_float(expression: &Expr) -> Option<Expr> {
    let float = |value: f64, ty: Type| {
        let value = if ty == Type::Float { f64::from(value as f32) } else { value };
        Expr { kind: ExprKind::Float(value), ty }
    };
    match &expression.kind {
        ExprKind::Convert(operand) if mwcc_iro::is_float(expression.ty) => match operand.kind {
            ExprKind::Float(value) => Some(float(value, expression.ty)),
            ExprKind::Int(value) => Some(float(value as f64, expression.ty)),
            _ => None,
        },
        // A floating literal converted to an integer truncates toward zero.
        ExprKind::Convert(operand) if expression.ty == Type::Int || expression.ty == Type::Short
            || expression.ty == Type::Char || expression.ty == Type::UnsignedChar
            || expression.ty == Type::UnsignedShort =>
        {
            let ExprKind::Float(value) = operand.kind else { return None };
            if !(-2147483648.0..2147483648.0).contains(&value) {
                return None;
            }
            let truncated = value.trunc() as i64;
            let narrowed = match expression.ty {
                Type::Short => i64::from(truncated as i16),
                Type::UnsignedShort => i64::from(truncated as u16),
                Type::Char => i64::from(truncated as i8),
                Type::UnsignedChar => i64::from(truncated as u8),
                _ => truncated,
            };
            Some(Expr { kind: ExprKind::Int(narrowed), ty: expression.ty })
        }
        // Division by a power of two multiplies by its exact reciprocal.
        ExprKind::Binary(BinaryOp::Divide, left, right) if mwcc_iro::is_float(expression.ty) => {
            let ExprKind::Float(b) = right.kind else { return None };
            if !matches!(left.kind, ExprKind::Float(_)) {
                let reciprocal = 1.0 / b;
                let exact = b != 0.0 && b.is_finite() && reciprocal.is_finite()
                    && (b.to_bits() & 0x000f_ffff_ffff_ffff) == 0
                    && (expression.ty == Type::Double || f64::from(reciprocal as f32) == reciprocal)
                    && reciprocal.is_normal();
                return exact.then(|| {
                    Expr::binary(BinaryOp::Multiply, (**left).clone(), float(reciprocal, expression.ty), expression.ty)
                });
            }
            let ExprKind::Float(a) = left.kind else { unreachable!() };
            (b != 0.0).then(|| float(a / b, expression.ty))
        }
        ExprKind::Binary(op, left, right) if mwcc_iro::is_float(expression.ty) => {
            let (ExprKind::Float(a), ExprKind::Float(b)) = (&left.kind, &right.kind) else { return None };
            let value = match op {
                BinaryOp::Add => a + b,
                BinaryOp::Subtract => a - b,
                BinaryOp::Multiply => a * b,

                _ => return None,
            };
            Some(float(value, expression.ty))
        }
        _ => None,
    }
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
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Var(_)
        | ExprKind::Global(_)
        | ExprKind::GlobalAddress(_)
        | ExprKind::LocalAddress(_)
        | ExprKind::StringAddress(_) => true,
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
                Idiom::Absolute(_) | Idiom::Insert { .. } => Type::Int,
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
        Idiom::Insert { .. } => return None,
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
            Stmt::Switch { arms, .. } => {
                for arm in arms {
                    rewrite_selects(function, arm, false);
                }
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
            Idiom::Absolute(_) | Idiom::Insert { .. } => Type::Int,
            Idiom::Masked { value, .. } => mwcc_iro::promote(value.ty),
        };
        return Some(vec![destination.assign(Expr { kind: ExprKind::Idiom(idiom), ty })]);
    }
    let simple = simple_condition(condition);
    // A floating comparison's truth value is still taken directly.
    let float_truth = matches!(&condition.kind, ExprKind::Binary(op, left, _)
        if op.is_comparison() && *op != BinaryOp::NotEqual && mwcc_iro::is_float(left.ty))
        && when_true.as_int() == Some(1)
        && when_false.as_int() == Some(0)
        && std::env::var_os("MWCC_IRO_NO_FLOAT_TRUTH").is_none();
    if simple || float_truth {
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
        // MWCC keeps the branches of a floating comparison.
        ExprKind::Binary(op, left, _) if op.is_comparison() && mwcc_iro::is_float(left.ty) => false,
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
                        // Extending a narrower value fills stored bytes.
                        || width(operand.ty) < stored
                        // A conversion from floating point is never dead.
                        || mwcc_iro::is_float(operand.ty)
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
            Stmt::Switch { arms, .. } => {
                for arm in arms {
                    stores(arm);
                }
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
            strings: Vec::new(),
            name: "f".into(),
            return_type: Type::Int,
            variables: (0..2)
                .map(|i| Variable { name: format!("p{i}"), ty: Type::Int, kind: VariableKind::Parameter, frame: None })
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

/// A scaled index with a constant addend, `(i ± k) * s` (or `i ± k`):
/// the variable part `i * s` and the constant `±k * s`.
fn constant_index_part(index: &Expr) -> Option<(Expr, i64)> {
    let (inner, size) = match &index.kind {
        ExprKind::Binary(BinaryOp::Multiply, inner, size) => (inner.as_ref(), size.as_int()?),
        _ => (index, 1),
    };
    let ExprKind::Binary(op @ (BinaryOp::Add | BinaryOp::Subtract), variable, addend) = &inner.kind else { return None };
    let addend = addend.as_int()?;
    if variable.as_int().is_some() {
        return None;
    }
    let constant = if *op == BinaryOp::Subtract { -addend } else { addend } * size;
    let variable = if size == 1 {
        (**variable).clone()
    } else {
        Expr::binary(BinaryOp::Multiply, (**variable).clone(), Expr::int(size), index.ty)
    };
    Some((variable, constant))
}

/// Forward substitution of offset addresses: a local assigned once as
/// `v + k` (with `v` never assigned) is replaced by `v + k` at every use,
/// so accesses through it fold `k` into their displacement.
pub fn forward_offsets(function: &mut Function) {
    fn count_assignments(body: &[Stmt], counts: &mut [usize]) {
        for statement in body {
            match statement {
                Stmt::Assign { variable, .. } => counts[*variable] += 1,
                Stmt::If { then_body, else_body, .. } => {
                    count_assignments(then_body, counts);
                    count_assignments(else_body, counts);
                }
                Stmt::Loop { body, step, .. } => {
                    // A definition inside a loop runs repeatedly.
                    let mut inner = vec![0; counts.len()];
                    count_assignments(body, &mut inner);
                    count_assignments(step, &mut inner);
                    for (count, extra) in counts.iter_mut().zip(inner) {
                        *count += extra * 2;
                    }
                }
                Stmt::Switch { arms, .. } => {
                    for arm in arms {
                        count_assignments(arm, counts);
                    }
                }
                _ => {}
            }
        }
    }
    fn definitions(body: &[Stmt], out: &mut Vec<(VarId, Expr)>) {
        for statement in body {
            match statement {
                Stmt::Assign { variable, value } => out.push((*variable, value.clone())),
                Stmt::If { then_body, else_body, .. } => {
                    definitions(then_body, out);
                    definitions(else_body, out);
                }
                Stmt::Switch { arms, .. } => {
                    for arm in arms {
                        definitions(arm, out);
                    }
                }
                _ => {}
            }
        }
    }
    fn remove_definitions(body: &mut Vec<Stmt>, forwarded: &HashMap<VarId, Expr>) {
        body.retain(|statement| !matches!(statement, Stmt::Assign { variable, .. } if forwarded.contains_key(variable)));
        for statement in body.iter_mut() {
            match statement {
                Stmt::If { then_body, else_body, .. } => {
                    remove_definitions(then_body, forwarded);
                    remove_definitions(else_body, forwarded);
                }
                Stmt::Switch { arms, .. } => {
                    for arm in arms {
                        remove_definitions(arm, forwarded);
                    }
                }
                Stmt::Loop { body, step, .. } => {
                    remove_definitions(body, forwarded);
                    remove_definitions(step, forwarded);
                }
                _ => {}
            }
        }
    }
    let mut counts = vec![0; function.variables.len()];
    count_assignments(&function.body, &mut counts);
    let mut defs = Vec::new();
    definitions(&function.body, &mut defs);
    let mut forwarded: HashMap<VarId, Expr> = HashMap::new();
    for (variable, value) in defs {
        if counts[variable] != 1 || function.variables[variable].kind != VariableKind::Local {
            continue;
        }
        let ExprKind::Binary(BinaryOp::Add, base, offset) = &value.kind else { continue };
        let (ExprKind::Var(source), Some(_)) = (&base.kind, offset.as_int()) else { continue };
        if counts[*source] != 0 || !pointer_like(value.ty) || !pointer_like(base.ty) {
            continue;
        }
        forwarded.insert(variable, value.clone());
    }
    // Only addresses: every use must sit inside a load or store address.
    fn value_uses(expression: &Expr, in_address: bool, out: &mut Vec<VarId>) {
        match &expression.kind {
            ExprKind::Var(id) if !in_address => out.push(*id),
            ExprKind::Load { base, index, .. } => {
                value_uses(base, true, out);
                if let Some(index) = index {
                    value_uses(index, false, out);
                }
            }
            ExprKind::Binary(BinaryOp::Add | BinaryOp::Subtract, left, right) if in_address => {
                value_uses(left, pointer_like(left.ty), out);
                value_uses(right, pointer_like(right.ty), out);
            }
            _ => {
                let mut copy = expression.clone();
                children(&mut copy, &mut |child| value_uses(child, false, out));
            }
        }
    }
    fn statement_value_uses(body: &mut [Stmt], out: &mut Vec<VarId>) {
        for statement in body.iter_mut() {
            match statement {
                Stmt::Store { place: mwcc_iro::Place::Memory { base, index, .. }, value, .. } => {
                    value_uses(base, true, out);
                    if let Some(index) = index {
                        value_uses(index, false, out);
                    }
                    value_uses(value, false, out);
                }
                Stmt::If { condition, then_body, else_body } => {
                    value_uses(condition, false, out);
                    statement_value_uses(then_body, out);
                    statement_value_uses(else_body, out);
                }
                Stmt::Loop { condition, body, step, .. } => {
                    if let Some(condition) = condition {
                        value_uses(condition, false, out);
                    }
                    statement_value_uses(body, out);
                    statement_value_uses(step, out);
                }
                Stmt::Switch { value, arms, .. } => {
                    value_uses(value, false, out);
                    for arm in arms {
                        statement_value_uses(arm, out);
                    }
                }
                other => for_each_expression(std::slice::from_mut(other), &mut |e| value_uses(e, false, out)),
            }
        }
    }
    let mut escaping = Vec::new();
    statement_value_uses(&mut function.body, &mut escaping);
    forwarded.retain(|variable, _| !escaping.contains(variable));
    if forwarded.is_empty() {
        return;
    }
    remove_definitions(&mut function.body, &forwarded);
    fn substitute(expression: &mut Expr, forwarded: &HashMap<VarId, Expr>) {
        if let ExprKind::Var(id) = expression.kind {
            if let Some(value) = forwarded.get(&id) {
                let ty = expression.ty;
                *expression = Expr { ty, ..value.clone() };
                return;
            }
        }
        children(expression, &mut |child| substitute(child, forwarded));
    }
    for_each_expression(&mut function.body, &mut |expression| substitute(expression, &forwarded));
}
