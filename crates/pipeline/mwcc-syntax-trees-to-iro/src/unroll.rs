//! Partial loop unrolling (`-O4`): a counted loop `for (i = i0; i < T; i++)`
//! too long to unroll completely runs `F` copies of its body per pass of a
//! count-register loop, and the iterations left over run in a second one.

use mwcc_iro::{BinaryOp, Expr, ExprKind, Function, Stmt, VarId};

use crate::passes::substitute;

pub fn unroll_partially(function: &mut Function) {
    let mut body = std::mem::take(&mut function.body);
    statements(&mut body);
    function.body = body;
}

fn statements(body: &mut Vec<Stmt>) {
    for statement in body.iter_mut() {
        match statement {
            Stmt::If { then_body, else_body, .. } => {
                statements(then_body);
                statements(else_body);
            }
            Stmt::Loop { body, .. } => statements(body),
            Stmt::Switch { arms, .. } => arms.iter_mut().for_each(statements),
            _ => {}
        }
    }
    let mut index = 1;
    while index < body.len() {
        let rest = &body[index + 1..];
        let Some(replacement) = unrolled(&body[index - 1], &body[index], &|variable| reads(rest, variable) > 0) else {
            index += 1;
            continue;
        };
        let count = replacement.len();
        body.splice(index..=index, replacement);
        index += count;
    }
}

/// The statements replacing `statement`, a loop started by `before`.
fn unrolled(before: &Stmt, statement: &Stmt, live_after: &dyn Fn(VarId) -> bool) -> Option<Vec<Stmt>> {
    let Stmt::Assign { variable, value: start } = before else { return None };
    let start = start.as_int()?;
    let Stmt::Loop { test_first: true, condition: Some(condition), body, step, effects } = statement else { return None };
    if !effects.is_empty() {
        return None;
    }
    let variable = *variable;
    let [Stmt::Assign { variable: stepped, value: increment }] = step.as_slice() else { return None };
    if *stepped != variable {
        return None;
    }
    // `i++` up to `i < end`, or `i--` down to `i != 0` / `i > 0`.
    let direction = match &increment.kind {
        ExprKind::Binary(BinaryOp::Add, base, one) if base.as_var() == Some(variable) => one.as_int()?,
        ExprKind::Binary(BinaryOp::Subtract, base, one) if base.as_var() == Some(variable) => -one.as_int()?,
        _ => return None,
    };
    let ExprKind::Binary(op, left, right) = &condition.kind else { return None };
    let (Some(tested), Some(end)) = (left.as_var(), right.as_int()) else { return None };
    if tested != variable {
        return None;
    }
    let trips = match (direction, op) {
        (1, BinaryOp::Less) => end - start,
        (-1, BinaryOp::NotEqual | BinaryOp::Greater) if end == 0 => start,
        _ => return None,
    };
    if trips < 1 {
        return None;
    }
    // A straight-line, call-free body that leaves the induction alone.
    let plain = |statement: &Stmt| {
        !format!("{statement:?}").contains("Call {")
            && match statement {
                Stmt::Assign { variable: assigned, .. } => *assigned != variable,
                Stmt::Store { .. } => true,
                _ => false,
            }
    };
    if body.is_empty() || !body.iter().all(plain) {
        return None;
    }
    // Other induction variables: `v = v + c`, assigned once.
    let inductions: Vec<(usize, VarId, i64)> = body
        .iter()
        .enumerate()
        .filter_map(|(position, statement)| match statement {
            Stmt::Assign { variable: v, value } => match &value.kind {
                ExprKind::Binary(BinaryOp::Add, left, right)
                    if left.as_var() == Some(*v)
                        && body.iter().filter(|s| matches!(s, Stmt::Assign { variable: w, .. } if w == v)).count() == 1 =>
                {
                    right.as_int().map(|c| (position, *v, c))
                }
                _ => None,
            },
            _ => None,
        })
        .collect();
    let cost: i64 = body
        .iter()
        .enumerate()
        .filter(|(position, _)| !inductions.iter().any(|(p, ..)| p == position))
        .map(|(_, statement)| match statement {
            Stmt::Assign { value, .. } | Stmt::Store { value, .. } => 1 + operations(value),
            _ => 1,
        })
        .sum::<i64>()
        .max(1);
    let limit = 48 / cost;
    if limit < 2 {
        return None;
    }
    let smooth = {
        let mut remaining = trips;
        for prime in [2, 3, 5, 7] {
            while remaining % prime == 0 {
                remaining /= prime;
            }
        }
        remaining == 1
    };
    let factor = if direction < 0 {
        // Counting down: completely to 10, else by the largest divisor
        // that is at most 10.
        if cost > 4 {
            return None;
        }
        if trips <= 10 { trips } else { (1..=10).rev().find(|f| trips % f == 0).unwrap_or(1) }
    } else if smooth && trips <= limit {
        trips
    } else if trips <= limit {
        trips - 1
    } else {
        limit.min(trips / 2)
    };
    let passes = trips / factor;
    let left_over = trips % factor;
    let ty = left.ty;
    let at = |k: i64| -> Expr {
        if k == 0 {
            Expr { kind: ExprKind::Var(variable), ty }
        } else {
            Expr::binary(BinaryOp::Add, Expr { kind: ExprKind::Var(variable), ty }, Expr::int(k), ty)
        }
    };
    // Copy `k` of the body: the induction reads `i + k` (a constant when
    // the copies run once), the others `v + k*c` (and `v + (k+1)*c` after
    // their own update, which goes).
    let copy = |k: i64, constant: bool| -> Vec<Stmt> {
        let mut out = Vec::new();
        for (position, statement) in body.iter().enumerate() {
            if inductions.iter().any(|(p, ..)| *p == position) {
                continue;
            }
            let mut statement = [statement.clone()];
            let value = if constant { Expr::int(start + direction * k) } else { at(direction * k) };
            substitute(&mut statement, variable, &value);
            for &(update, v, c) in &inductions {
                let steps = k + i64::from(position > update);
                if steps != 0 {
                    let current = Expr { kind: ExprKind::Var(v), ty: induction_type(body, v) };
                    let offset = Expr::binary(BinaryOp::Add, current.clone(), Expr::int(steps * c), current.ty);
                    substitute(&mut statement, v, &offset);
                }
            }
            let [mut statement] = statement;
            displaced(&mut statement);
            out.push(statement);
        }
        out
    };
    let advance = |v: VarId, by: i64, vty| Stmt::Assign {
        variable: v,
        value: Expr::binary(BinaryOp::Add, Expr { kind: ExprKind::Var(v), ty: vty }, Expr::int(by), vty),
    };
    let keep_induction = left_over > 0 || live_after(variable);
    let mut out = Vec::new();
    let mut main = Vec::new();
    for k in 0..factor {
        main.extend(copy(k, passes == 1));
    }
    for &(_, v, c) in &inductions {
        main.push(advance(v, factor * c, induction_type(body, v)));
    }
    if passes == 1 {
        out.extend(main);
        if keep_induction {
            out.push(Stmt::Assign { variable, value: Expr { kind: ExprKind::Int(start + direction * factor), ty } });
        }
    } else {
        // (Strength reduction still needs the induction; dead, it goes.)
        main.push(advance(variable, direction * factor, ty));
        out.push(Stmt::Counted { count: Expr::int(passes), guard: None, body: main });
    }
    if left_over > 0 {
        let mut rest = body.clone();
        rest.push(advance(variable, 1, ty));
        out.push(Stmt::Counted {
            count: Expr::binary(BinaryOp::Subtract, Expr::int(end), Expr { kind: ExprKind::Var(variable), ty }, ty),
            guard: Some(condition.clone()),
            body: rest,
        });
    }
    Some(out)
}

fn induction_type(body: &[Stmt], variable: VarId) -> mwcc_iro::Type {
    body.iter()
        .find_map(|statement| match statement {
            Stmt::Assign { variable: v, value } if *v == variable => Some(value.ty),
            _ => None,
        })
        .unwrap_or(mwcc_iro::Type::Int)
}

fn operations(e: &Expr) -> i64 {
    match &e.kind {
        ExprKind::Load { .. } => 1,
        ExprKind::Binary(_, left, right) => 1 + operations(left) + operations(right),
        ExprKind::Unary(_, operand) | ExprKind::Convert(operand) => 1 + operations(operand),
        _ => 0,
    }
}

/// Reads of `variable` (conservatively by its listing).
fn reads(body: &[Stmt], variable: VarId) -> usize {
    let needle = format!("Var({variable})");
    body.iter().map(|statement| format!("{statement:?}").matches(&needle).count()).sum()
}

/// `*(p + k)` is `p` displaced by `k`.
fn displaced(statement: &mut Stmt) {
    fn fold_into(base: &mut Box<Expr>, offset: &mut i32) {
        while let ExprKind::Binary(BinaryOp::Add, inner, constant) = &base.kind {
            let (Some(k), true) = (constant.as_int(), mwcc_iro::element_size(inner.ty).is_some() || matches!(inner.ty, mwcc_iro::Type::Pointer(_))) else { break };
            let Ok(k) = i32::try_from(k) else { break };
            *offset += k;
            let ty = base.ty;
            **base = Expr { ty, ..(**inner).clone() };
        }
    }
    // `(v + k) * s` as an index: `v * s` displaced by `k * s`.
    fn fold_index(index: &mut Option<Box<Expr>>, offset: &mut i32) {
        let Some(scaled) = index.as_deref_mut() else { return };
        let ExprKind::Binary(BinaryOp::Multiply, sum, scale) = &mut scaled.kind else { return };
        let Some(s) = scale.as_int() else { return };
        let ExprKind::Binary(BinaryOp::Add, variable, constant) = &sum.kind else { return };
        let Some(k) = constant.as_int() else { return };
        let Ok(displacement) = i32::try_from(k * s) else { return };
        *offset += displacement;
        let ty = sum.ty;
        **sum = Expr { ty, ..(**variable).clone() };
    }
    fn expression(e: &mut Expr) {
        if let ExprKind::Load { base, index: None, offset } = &mut e.kind {
            fold_into(base, offset);
        }
        if let ExprKind::Load { index, offset, .. } = &mut e.kind {
            fold_index(index, offset);
        }
        match &mut e.kind {
            ExprKind::Load { base, index, .. } => {
                expression(base);
                if let Some(index) = index {
                    expression(index);
                }
            }
            ExprKind::Unary(_, operand) | ExprKind::Convert(operand) => expression(operand),
            ExprKind::Binary(_, left, right) => {
                expression(left);
                expression(right);
            }
            ExprKind::Select { condition, when_true, when_false } => {
                expression(condition);
                expression(when_true);
                expression(when_false);
            }
            ExprKind::Call { arguments, .. } => arguments.iter_mut().for_each(expression),
            _ => {}
        }
    }
    if let Stmt::Store { place: mwcc_iro::Place::Memory { base, index: None, offset }, .. } = statement {
        fold_into(base, offset);
    }
    if let Stmt::Store { place: mwcc_iro::Place::Memory { index, offset, .. }, .. } = statement {
        fold_index(index, offset);
    }
    crate::passes::for_each_expression(std::slice::from_mut(statement), &mut expression);
}
