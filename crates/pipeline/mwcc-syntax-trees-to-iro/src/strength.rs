//! Loop strength reduction (`-O3`/`-O4`): an address `base + i*k` inside a
//! loop whose induction variable `i` steps by a constant becomes a cursor
//! variable, set before the loop and advanced with `i` in its step.

use mwcc_iro::{BinaryOp, Expr, ExprKind, Function, Place, Stmt, Type, VarId, VariableKind};

/// `fold_start`: a constant start of the induction variable folds into
/// the cursors' starts (GC/1.0-1.2.5n compute `base + i*k` after it).
pub fn strength_reduce(function: &mut Function, fold_start: bool) {
    let first_cursor = function.variables.len();
    let mut body = std::mem::take(&mut function.body);
    statements(&mut body, function, fold_start);
    if std::env::var_os("MWCC_IRO_NO_CURSOR_COPIES").is_none() {
        let whole = body.clone();
        propagate_cursor_copies(&mut body, &whole, first_cursor, function);
    }
    function.body = body;
}

/// `a = cursor` in a loop body, `a` assigned nowhere else and read only in
/// that body: its reads read the cursor (copy propagation).
fn propagate_cursor_copies(body: &mut [Stmt], whole: &[Stmt], first_cursor: VarId, function: &Function) {
    for statement in body.iter_mut() {
        match statement {
            Stmt::If { then_body, else_body, .. } => {
                propagate_cursor_copies(then_body, whole, first_cursor, function);
                propagate_cursor_copies(else_body, whole, first_cursor, function);
            }
            Stmt::Switch { arms, .. } => {
                arms.iter_mut().for_each(|arm| propagate_cursor_copies(arm, whole, first_cursor, function))
            }
            Stmt::Loop { body: inner, .. } => {
                propagate_cursor_copies(inner, whole, first_cursor, function);
                let mut position = 0;
                while position < inner.len() {
                    let copy = match &inner[position] {
                        Stmt::Assign { variable, value } => match value.kind {
                            ExprKind::Var(cursor) if cursor >= first_cursor && *variable < first_cursor => {
                                Some((*variable, cursor))
                            }
                            _ => None,
                        },
                        _ => None,
                    };
                    let Some((variable, cursor)) = copy else {
                        position += 1;
                        continue;
                    };
                    let local = &function.variables[variable];
                    let eligible = local.frame.is_none()
                        && !local.volatile
                        && count_assignments(whole, variable) == 1
                        && uses(whole, variable) == uses(&inner[position + 1..], variable)
                        && count_assignments(&inner[position + 1..], cursor) == 0;
                    if !eligible {
                        position += 1;
                        continue;
                    }
                    let value = Expr { kind: ExprKind::Var(cursor), ty: local.ty };
                    crate::passes::substitute(&mut inner[position + 1..], variable, &value);
                    inner.remove(position);
                }
            }
            _ => {}
        }
    }
}

/// Reads of `variable`.
fn uses(body: &[Stmt], variable: VarId) -> usize {
    fn expression(e: &Expr, variable: VarId) -> usize {
        match &e.kind {
            ExprKind::Var(id) | ExprKind::LocalAddress(id) => usize::from(*id == variable),
            ExprKind::Load { base, index, .. } => {
                expression(base, variable) + index.as_deref().map_or(0, |index| expression(index, variable))
            }
            ExprKind::Unary(_, operand) | ExprKind::Convert(operand) => expression(operand, variable),
            ExprKind::Binary(_, left, right) => expression(left, variable) + expression(right, variable),
            ExprKind::Select { condition, when_true, when_false } => {
                expression(condition, variable) + expression(when_true, variable) + expression(when_false, variable)
            }
            ExprKind::Call { arguments, .. } => arguments.iter().map(|argument| expression(argument, variable)).sum(),
            // (Conservatively, an idiom reads everything.)
            ExprKind::Idiom(_) => 1,
            _ => 0,
        }
    }
    body.iter()
        .map(|statement| match statement {
            Stmt::Assign { value, .. } | Stmt::Eval(value) | Stmt::SetReturn(value) => expression(value, variable),
            Stmt::Return(value) => value.as_ref().map_or(0, |value| expression(value, variable)),
            Stmt::Store { place, value, .. } => {
                expression(value, variable)
                    + match place {
                        Place::Memory { base, index, .. } => {
                            expression(base, variable) + index.as_deref().map_or(0, |index| expression(index, variable))
                        }
                        Place::Global(_) => 0,
                    }
            }
            Stmt::If { condition, then_body, else_body } => {
                expression(condition, variable) + uses(then_body, variable) + uses(else_body, variable)
            }
            Stmt::Loop { condition, body, step, effects, .. } => {
                condition.as_ref().map_or(0, |condition| expression(condition, variable))
                    + uses(body, variable)
                    + uses(step, variable)
                    + uses(effects, variable)
            }
            Stmt::Switch { value, arms, .. } => expression(value, variable) + arms.iter().map(|arm| uses(arm, variable)).sum::<usize>(),
            Stmt::Break | Stmt::Continue | Stmt::Goto(_) | Stmt::Label(_) => 0,
        })
        .sum()
}

fn statements(body: &mut Vec<Stmt>, function: &mut Function, fold_start: bool) {
    let mut index = 0;
    while index < body.len() {
        // Inner loops first.
        match &mut body[index] {
            Stmt::If { then_body, else_body, .. } => {
                statements(then_body, function, fold_start);
                statements(else_body, function, fold_start);
            }
            Stmt::Loop { body: inner, .. } => statements(inner, function, fold_start),
            Stmt::Switch { arms, .. } => arms.iter_mut().for_each(|arm| statements(arm, function, fold_start)),
            _ => {}
        }
        let initial = match index.checked_sub(1).map(|previous| &body[previous]) {
            Some(Stmt::Assign { variable, value }) => value.as_int().map(|value| (*variable, value)),
            _ => None,
        };
        let (inits, folded) = match &mut body[index] {
            Stmt::Loop { condition, body: inner, step, effects, .. } => {
                reduce_loop(condition.as_mut(), inner, step, effects, initial, fold_start, function)
            }
            _ => (Vec::new(), false),
        };
        let count = inits.len();
        // (Before the induction variable's own initialization when they
        // do not read it.)
        let at = if folded { index - 1 } else { index };
        for (offset, init) in inits.into_iter().enumerate() {
            body.insert(at + offset, init);
        }
        index += count + 1;
    }
}

/// A cursor: the invariant base, the byte stride per unit of the induction
/// variable, and the cursor variable.
struct Cursor {
    base: Expr,
    key: String,
    stride: i64,
    variable: VarId,
}

fn reduce_loop(
    _condition: Option<&mut Expr>,
    body: &mut [Stmt],
    step: &mut Vec<Stmt>,
    effects: &mut [Stmt],
    initial: Option<(VarId, i64)>,
    fold_start: bool,
    function: &mut Function,
) -> (Vec<Stmt>, bool) {
    let mut inits = Vec::new();
    let mut folded = true;
    // Induction variables: `v = v + c` in the step, assigned nowhere else.
    let inductions: Vec<(usize, VarId, i64)> = step
        .iter()
        .enumerate()
        .filter_map(|(position, statement)| match statement {
            Stmt::Assign { variable, value } => match &value.kind {
                ExprKind::Binary(BinaryOp::Add, left, right)
                    if left.as_var() == Some(*variable) && right.as_int().is_some() =>
                {
                    Some((position, *variable, right.as_int().expect("checked")))
                }
                _ => None,
            },
            _ => None,
        })
        .collect();
    // Each cursor advances just before its induction variable.
    let mut shift = 0;
    for (position, induction, increment) in inductions {
        let variable = &function.variables[induction];
        if variable.frame.is_some() || variable.volatile || !mwcc_iro::is_general_word(variable.ty) {
            continue;
        }
        let assignments = count_assignments(body, induction)
            + count_assignments(step, induction)
            + count_assignments(effects, induction);
        if assignments != 1 {
            continue;
        }
        let mut cursors: Vec<Cursor> = Vec::new();
        let mut assigned = Vec::new();
        collect_assigned(body, &mut assigned);
        collect_assigned(step, &mut assigned);
        collect_assigned(effects, &mut assigned);
        rewrite_statements(body, induction, &assigned, &mut cursors, function);
        if cursors.is_empty() {
            continue;
        }
        for cursor in &cursors {
            let ty = cursor.base.ty;
            // (GC/1.0-1.2.5n fold it only while the loop still reads the
            // induction variable's value.)
            let fold = fold_start || uses(body, induction) > 0;
            let start = match initial {
                Some((variable, value)) if variable == induction && fold => {
                    let offset = value * cursor.stride;
                    if offset == 0 {
                        cursor.base.clone()
                    } else {
                        Expr::binary(BinaryOp::Add, cursor.base.clone(), Expr::int(offset), ty)
                    }
                }
                _ => {
                    folded = false;
                    Expr::binary(
                    BinaryOp::Add,
                    cursor.base.clone(),
                    Expr::binary(
                        BinaryOp::Multiply,
                        Expr { kind: ExprKind::Var(induction), ty: function.variables[induction].ty },
                        Expr::int(cursor.stride),
                        Type::Int,
                    ),
                    ty,
                )
                }
            };
            inits.push(Stmt::Assign { variable: cursor.variable, value: start });
            let advance = Expr::binary(
                BinaryOp::Add,
                Expr { kind: ExprKind::Var(cursor.variable), ty },
                Expr::int(increment * cursor.stride),
                ty,
            );
            step.insert(position + shift, Stmt::Assign { variable: cursor.variable, value: advance });
            shift += 1;
        }
    }
    let folded = folded && !inits.is_empty();
    (inits, folded)
}

fn count_assignments(body: &[Stmt], variable: VarId) -> usize {
    body.iter()
        .map(|statement| match statement {
            Stmt::Assign { variable: assigned, .. } => usize::from(*assigned == variable),
            Stmt::If { then_body, else_body, .. } => count_assignments(then_body, variable) + count_assignments(else_body, variable),
            Stmt::Loop { body, step, effects, .. } => {
                count_assignments(body, variable) + count_assignments(step, variable) + count_assignments(effects, variable)
            }
            Stmt::Switch { arms, .. } => arms.iter().map(|arm| count_assignments(arm, variable)).sum(),
            _ => 0,
        })
        .sum()
}

fn collect_assigned(body: &[Stmt], out: &mut Vec<VarId>) {
    for statement in body {
        match statement {
            Stmt::Assign { variable, .. } => out.push(*variable),
            Stmt::If { then_body, else_body, .. } => {
                collect_assigned(then_body, out);
                collect_assigned(else_body, out);
            }
            Stmt::Loop { body, step, effects, .. } => {
                collect_assigned(body, out);
                collect_assigned(step, out);
                collect_assigned(effects, out);
            }
            Stmt::Switch { arms, .. } => arms.iter().for_each(|arm| collect_assigned(arm, out)),
            _ => {}
        }
    }
}

/// A loop-invariant base: global addresses, constants and register
/// variables the loop never assigns.
fn is_invariant(expression: &Expr, assigned: &[VarId], function: &Function) -> bool {
    match &expression.kind {
        ExprKind::GlobalAddress(_) | ExprKind::Int(_) => true,
        ExprKind::Var(id) => {
            let variable = &function.variables[*id];
            variable.frame.is_none() && !variable.volatile && !assigned.contains(id)
        }
        ExprKind::Binary(BinaryOp::Add | BinaryOp::Subtract, left, right) => {
            is_invariant(left, assigned, function) && is_invariant(right, assigned, function)
        }
        _ => false,
    }
}

/// `index` as `induction * stride` (bytes).
fn scaled(index: &Expr, induction: VarId) -> Option<i64> {
    match &index.kind {
        ExprKind::Var(id) if *id == induction => Some(1),
        ExprKind::Binary(BinaryOp::Multiply, left, right) => Some(scaled(left, induction)? * right.as_int()?),
        ExprKind::Binary(BinaryOp::ShiftLeft, left, right) => {
            Some(scaled(left, induction)? << right.as_int().filter(|shift| (0..31).contains(shift))?)
        }
        _ => None,
    }
}

fn cursor_for(base: &Expr, stride: i64, cursors: &mut Vec<Cursor>, function: &mut Function) -> VarId {
    let key = format!("{:?}", base);
    if let Some(cursor) = cursors.iter().find(|cursor| cursor.key == key && cursor.stride == stride) {
        return cursor.variable;
    }
    let variable = function.add_temporary(base.ty);
    debug_assert_eq!(function.variables[variable].kind, VariableKind::Temporary);
    cursors.push(Cursor { base: base.clone(), key, stride, variable });
    variable
}

fn rewrite_statements(body: &mut [Stmt], induction: VarId, assigned: &[VarId], cursors: &mut Vec<Cursor>, function: &mut Function) {
    for statement in body {
        match statement {
            Stmt::Assign { value, .. } | Stmt::Eval(value) | Stmt::SetReturn(value) => {
                rewrite(value, induction, assigned, cursors, function)
            }
            Stmt::Return(value) => {
                if let Some(value) = value {
                    rewrite(value, induction, assigned, cursors, function);
                }
            }
            Stmt::Store { place, value, .. } => {
                if let Place::Memory { base, index, .. } = place {
                    match index.as_deref().and_then(|index| scaled(index, induction)) {
                        Some(stride) if is_invariant(base, assigned, function) => {
                            let cursor = cursor_for(base, stride, cursors, function);
                            let ty = base.ty;
                            **base = Expr { kind: ExprKind::Var(cursor), ty };
                            *index = None;
                        }
                        _ => {
                            rewrite(base, induction, assigned, cursors, function);
                            if let Some(index) = index {
                                rewrite(index, induction, assigned, cursors, function);
                            }
                        }
                    }
                }
                rewrite(value, induction, assigned, cursors, function);
            }
            Stmt::If { condition, then_body, else_body } => {
                rewrite(condition, induction, assigned, cursors, function);
                rewrite_statements(then_body, induction, assigned, cursors, function);
                rewrite_statements(else_body, induction, assigned, cursors, function);
            }
            Stmt::Loop { condition, body, step, effects, .. } => {
                if let Some(condition) = condition {
                    rewrite(condition, induction, assigned, cursors, function);
                }
                rewrite_statements(body, induction, assigned, cursors, function);
                rewrite_statements(step, induction, assigned, cursors, function);
                rewrite_statements(effects, induction, assigned, cursors, function);
            }
            Stmt::Switch { value, arms, .. } => {
                rewrite(value, induction, assigned, cursors, function);
                for arm in arms {
                    rewrite_statements(arm, induction, assigned, cursors, function);
                }
            }
            Stmt::Break | Stmt::Continue | Stmt::Goto(_) | Stmt::Label(_) => {}
        }
    }
}

fn rewrite(expression: &mut Expr, induction: VarId, assigned: &[VarId], cursors: &mut Vec<Cursor>, function: &mut Function) {
    match &mut expression.kind {
        ExprKind::Load { base, index, .. } => match index.as_deref().and_then(|index| scaled(index, induction)) {
            Some(stride) if is_invariant(base, assigned, function) => {
                let cursor = cursor_for(base, stride, cursors, function);
                let ty = base.ty;
                **base = Expr { kind: ExprKind::Var(cursor), ty };
                *index = None;
            }
            _ => {
                rewrite(base, induction, assigned, cursors, function);
                if let Some(index) = index {
                    rewrite(index, induction, assigned, cursors, function);
                }
            }
        },
        // `base + induction*stride` as an address.
        ExprKind::Binary(BinaryOp::Add, left, right)
            if mwcc_iro::element_size(expression.ty).is_some()
                && scaled(right, induction).is_some()
                && is_invariant(left, assigned, function) =>
        {
            let stride = scaled(right, induction).expect("checked");
            let cursor = cursor_for(left, stride, cursors, function);
            expression.kind = ExprKind::Var(cursor);
        }
        _ => {
            let mut children: Vec<&mut Expr> = Vec::new();
            match &mut expression.kind {
                ExprKind::Unary(_, operand) | ExprKind::Convert(operand) => children.push(operand),
                ExprKind::Binary(_, left, right) => {
                    children.push(left);
                    children.push(right);
                }
                ExprKind::Select { condition, when_true, when_false } => {
                    children.push(condition);
                    children.push(when_true);
                    children.push(when_false);
                }
                ExprKind::Call { arguments, .. } => children.extend(arguments.iter_mut()),
                _ => {}
            }
            for child in children {
                rewrite(child, induction, assigned, cursors, function);
            }
        }
    }
}

/// Constant propagation (`-O3`/`-O4`): a local assigned once, at the top
/// level, an integer or a global's address is that value
/// wherever it is read.
pub fn propagate_constants(function: &mut Function) {
    let mut position = 0;
    while position < function.body.len() {
        let candidate = match &function.body[position] {
            Stmt::Assign { variable, value } if constant(value) => Some((*variable, value.clone())),
            _ => None,
        };
        let Some((variable, value)) = candidate else {
            position += 1;
            continue;
        };
        let local = &function.variables[variable];
        let eligible = local.kind == VariableKind::Local
            && local.frame.is_none()
            && !local.volatile
            && !local.raw
            && count_assignments(&function.body, variable) == 1
            && uses(&function.body[..position], variable) == 0
            // (An address used as a memory base stays in its variable.)
            && (value.as_int().is_some() || !based(&function.body, variable));
        if !eligible {
            position += 1;
            continue;
        }
        let value = Expr { kind: value.kind, ty: local.ty };
        crate::passes::substitute(&mut function.body[position + 1..], variable, &value);
        function.body.remove(position);
    }
}

/// (An address plus an offset stays in its variable.)
fn constant(value: &Expr) -> bool {
    matches!(value.kind, ExprKind::Int(_) | ExprKind::GlobalAddress(_))
}

/// Whether `variable` is the base of a load or store address.
fn based(body: &[Stmt], variable: VarId) -> bool {
    fn base_is(base: &Expr, variable: VarId) -> bool {
        match &base.kind {
            ExprKind::Var(id) => *id == variable,
            ExprKind::Binary(BinaryOp::Add | BinaryOp::Subtract, left, _) => base_is(left, variable),
            _ => false,
        }
    }
    fn expression(e: &Expr, variable: VarId) -> bool {
        match &e.kind {
            ExprKind::Load { base, index, .. } => {
                base_is(base, variable)
                    || expression(base, variable)
                    || index.as_deref().is_some_and(|index| expression(index, variable))
            }
            ExprKind::Unary(_, operand) | ExprKind::Convert(operand) => expression(operand, variable),
            ExprKind::Binary(_, left, right) => expression(left, variable) || expression(right, variable),
            ExprKind::Select { condition, when_true, when_false } => {
                expression(condition, variable) || expression(when_true, variable) || expression(when_false, variable)
            }
            ExprKind::Call { arguments, .. } => arguments.iter().any(|argument| expression(argument, variable)),
            ExprKind::Idiom(_) => true,
            _ => false,
        }
    }
    body.iter().any(|statement| match statement {
        Stmt::Assign { value, .. } | Stmt::Eval(value) | Stmt::SetReturn(value) => expression(value, variable),
        Stmt::Return(value) => value.as_ref().is_some_and(|value| expression(value, variable)),
        Stmt::Store { place, value, .. } => {
            expression(value, variable)
                || match place {
                    Place::Memory { base, index, .. } => {
                        base_is(base, variable)
                            || expression(base, variable)
                            || index.as_deref().is_some_and(|index| expression(index, variable))
                    }
                    Place::Global(_) => false,
                }
        }
        Stmt::If { condition, then_body, else_body } => {
            expression(condition, variable) || based(then_body, variable) || based(else_body, variable)
        }
        Stmt::Loop { condition, body, step, effects, .. } => {
            condition.as_ref().is_some_and(|condition| expression(condition, variable))
                || based(body, variable)
                || based(step, variable)
                || based(effects, variable)
        }
        Stmt::Switch { value, arms, .. } => expression(value, variable) || arms.iter().any(|arm| based(arm, variable)),
        Stmt::Break | Stmt::Continue | Stmt::Goto(_) | Stmt::Label(_) => false,
    })
}
