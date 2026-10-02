//! Partial loop unrolling (`-O4`): a counted loop `for (i = i0; i < T; i++)`
//! too long to unroll completely runs `F` copies of its body per pass of a
//! count-register loop, and the iterations left over run in a second one.

use mwcc_iro::{BinaryOp, Expr, ExprKind, Function, Stmt, VarId};

use crate::passes::substitute;

/// `unrolling`: copies per pass (an explicit speed goal); otherwise one.
pub fn unroll_partially(function: &mut Function, unrolling: bool) {
    let mut body = std::mem::take(&mut function.body);
    UNROLLING.with(|flag| flag.set(unrolling));
    statements(&mut body, function);
    function.body = body;
}

thread_local! {
    static UNROLLING: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

fn statements(body: &mut Vec<Stmt>, function: &mut Function) {
    for statement in body.iter_mut() {
        match statement {
            Stmt::If { then_body, else_body, .. } => {
                statements(then_body, function);
                statements(else_body, function);
            }
            Stmt::Loop { body, .. } => statements(body, function),
            Stmt::Switch { arms, .. } => arms.iter_mut().for_each(|arm| statements(arm, function)),
            _ => {}
        }
    }
    let mut index = 1;
    while index < body.len() {
        let rest = &body[index + 1..];
        let Some(replacement) = unrolled(&body[index - 1], &body[index], &|variable| reads(rest, variable) > 0, function) else {
            index += 1;
            continue;
        };
        let count = replacement.len();
        body.splice(index..=index, replacement);
        index += count;
    }
}

/// The statements replacing `statement`, a loop started by `before`.
fn unrolled(
    before: &Stmt,
    statement: &Stmt,
    live_after: &dyn Fn(VarId) -> bool,
    function: &mut Function,
) -> Option<Vec<Stmt>> {
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
    if left.as_var() != Some(variable) {
        return None;
    }
    // The bound: a constant, or (counting up) an invariant variable.
    let bound = right.as_var();
    let (end, trips) = match (right.as_int(), bound, direction, op) {
        (Some(end), _, 1, BinaryOp::Less) => (end, end - start),
        (Some(0), _, -1, BinaryOp::NotEqual | BinaryOp::Greater) => (0, start),
        (None, Some(_), 1, BinaryOp::Less) => (0, i64::MAX),
        _ => return None,
    };
    if trips < 1 {
        return None;
    }
    // A straight-line, call-free body that leaves the induction (and the
    // bound) alone.
    let plain = |statement: &Stmt| {
        !format!("{statement:?}").contains("Call {")
            && match statement {
                Stmt::Assign { variable: assigned, .. } => *assigned != variable && Some(*assigned) != bound,
                Stmt::Store { .. } => true,
                _ => false,
            }
    };
    // (Or an exit: `if (c) { ...; break; }` / `if (c) return v;`.)
    let exit = |statement: &Stmt| match statement {
        Stmt::If { then_body, else_body, .. } if else_body.is_empty() && !format!("{statement:?}").contains("Call {") => {
            matches!(then_body.last(), Some(Stmt::Break | Stmt::Return(_)))
                && then_body[..then_body.len() - 1].iter().all(plain)
        }
        _ => false,
    };
    if body.is_empty() || !body.iter().all(|statement| plain(statement) || exit(statement)) {
        return None;
    }
    let exits = body.iter().any(exit);
    let breaks = format!("{body:?}").contains("Break");
    if exits && std::env::var_os("MWCC_IRO_NO_EXIT_LOOPS").is_some() {
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
    if let Some(bound) = bound {
        let n = Expr { kind: ExprKind::Var(bound), ty: right.ty };
        let entered = Expr { kind: ExprKind::Binary(BinaryOp::Greater, Box::new(n.clone()), Box::new(Expr::int(start))), ty: condition.ty };
        let mut rest = body.clone();
        rest.push(advance(variable, 1, ty));
        let remainder = |guard: Expr| Stmt::Counted {
            count: Expr::binary(BinaryOp::Subtract, n.clone(), Expr { kind: ExprKind::Var(variable), ty }, ty),
            guard: Some(guard),
            body: rest.clone(),
        };
        // (A loop with exits is not unrolled.)
        if !UNROLLING.with(|flag| flag.get()) || exits {
            // `mtctr n - i0`, skipped unless `n > i0`.
            let count = if start == 0 { n.clone() } else { Expr::binary(BinaryOp::Subtract, n.clone(), Expr::int(start), ty) };
            return Some(vec![Stmt::Counted { count, guard: Some(entered), body: rest }]);
        }
        // Eight copies per pass while `n - i0 > 8`, the rest one at a time.
        let past = function.add_temporary(right.ty);
        let past_expr = Expr { kind: ExprKind::Var(past), ty: right.ty };
        let span = if start == 0 { n.clone() } else { Expr::binary(BinaryOp::Subtract, n.clone(), Expr::int(start), right.ty) };
        let mut main = Vec::new();
        for k in 0..8 {
            main.extend(copy(k, false));
        }
        for &(_, v, c) in &inductions {
            main.push(advance(v, 8 * c, induction_type(body, v)));
        }
        main.push(advance(variable, 8, ty));
        let unsigned = mwcc_iro::Type::UnsignedInt;
        let passes = Expr::binary(
            BinaryOp::ShiftRight,
            Expr::binary(BinaryOp::Add, Expr { ty: unsigned, ..past_expr.clone() }, Expr::int(7 - start), unsigned),
            Expr::int(3),
            unsigned,
        );
        let unrolled_loop = Stmt::Counted {
            count: passes,
            guard: Some(Expr { kind: ExprKind::Binary(BinaryOp::Greater, Box::new(past_expr.clone()), Box::new(Expr::int(start))), ty: condition.ty }),
            body: main,
        };
        let long = Expr { kind: ExprKind::Binary(BinaryOp::Greater, Box::new(span), Box::new(Expr::int(8))), ty: condition.ty };
        return Some(vec![Stmt::If {
            condition: entered,
            then_body: vec![
                Stmt::Assign { variable: past, value: Expr::binary(BinaryOp::Subtract, n.clone(), Expr::int(8), right.ty) },
                Stmt::If { condition: long, then_body: vec![unrolled_loop], else_body: Vec::new() },
                remainder(condition.clone()),
            ],
            else_body: Vec::new(),
        }]);
    }
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
    let factor = if !UNROLLING.with(|flag| flag.get()) {
        1
    } else if direction < 0 {
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
    // (With exits: completely or not at all, and a `break` only when the
    // induction is not read after.)
    let factor = if exits && (factor != trips || breaks && live_after(variable)) { 1 } else { factor };
    let passes = trips / factor;
    let left_over = trips % factor;
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
        // (A `break` leaves the copies.)
        if breaks {
            out.push(Stmt::Loop { test_first: false, condition: Some(Expr::int(0)), body: main, step: Vec::new(), effects: Vec::new() });
        } else {
            out.extend(main);
        }
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
            _ => crate::passes::children(e, &mut |child| expression(child)),
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
