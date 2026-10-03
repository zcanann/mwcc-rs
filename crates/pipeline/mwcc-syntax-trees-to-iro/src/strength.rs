//! Loop strength reduction (`-O3`/`-O4`): an address `base + i*k` inside a
//! loop whose induction variable `i` steps by a constant becomes a cursor
//! variable, set before the loop and advanced with `i` in its step.

use mwcc_iro::{BinaryOp, Expr, ExprKind, Function, Place, Stmt, Type, VarId, VariableKind};

/// `fold_start`: a constant start of the induction variable folds into
/// the cursors' starts (GC/1.0-1.2.5n compute `base + i*k` after it).
/// `cursors`: addresses become pointer cursors (with an explicit speed
/// goal); otherwise only constants are hoisted.
pub fn strength_reduce(function: &mut Function, fold_start: bool, cursors: bool) {
    let first_cursor = function.variables.len();
    let mut body = std::mem::take(&mut function.body);
    CURSORS.with(|flag| flag.set(cursors));
    KNOWN.with(|known| known.borrow_mut().clear());
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
            ExprKind::Idiom(mwcc_iro::Idiom::Update(_, left, right)) => expression(left, variable) + expression(right, variable),
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
            Stmt::Counted { count, guard, body } => {
                expression(count, variable)
                    + guard.as_ref().map_or(0, |guard| expression(guard, variable))
                    + uses(body, variable)
            }
            Stmt::Switch { value, arms, .. } => expression(value, variable) + arms.iter().map(|arm| uses(arm, variable)).sum::<usize>(),
            Stmt::Break | Stmt::Continue | Stmt::Goto(_) | Stmt::Label(_) => 0,
        })
        .sum()
}

thread_local! {
    static CURSORS: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

fn statements(body: &mut Vec<Stmt>, function: &mut Function, fold_start: bool) {
    // Constants known here: inherited, then those this list assigns.
    let mut known: Vec<(VarId, i64)> = KNOWN.with(|known| known.borrow().clone());
    let mut index = 0;
    while index < body.len() {
        // Inner loops first.
        let inherit = |known: &Vec<(VarId, i64)>| KNOWN.with(|cell| *cell.borrow_mut() = known.clone());
        match &mut body[index] {
            Stmt::If { then_body, else_body, .. } => {
                inherit(&known);
                statements(then_body, function, fold_start);
                inherit(&known);
                statements(else_body, function, fold_start);
            }
            Stmt::Loop { body: inner, .. } | Stmt::Counted { body: inner, .. } => {
                inherit(&Vec::new());
                LOOP_DEPTH.with(|depth| depth.set(depth.get() + 1));
                statements(inner, function, fold_start);
                LOOP_DEPTH.with(|depth| depth.set(depth.get() - 1));
            }
            Stmt::Switch { arms, .. } => {
                inherit(&Vec::new());
                arms.iter_mut().for_each(|arm| statements(arm, function, fold_start))
            }
            _ => {}
        }
        CONTEXT.with(|context| *context.borrow_mut() = known.clone());
        let initial = match index.checked_sub(1).map(|previous| &body[previous]) {
            Some(Stmt::Assign { variable, value }) => value.as_int().map(|value| (*variable, value)),
            _ => None,
        };
        // Constants the loop stores are set before it (loop code motion,
        // which runs before strength reduction).
        let mut hoisted = match &mut body[index] {
            Stmt::Loop { body: inner, .. } | Stmt::Counted { body: inner, .. }
                if std::env::var_os("MWCC_IRO_NO_HOIST_CONSTANTS").is_none() =>
            {
                hoist_stored_constants(inner, function)
            }
            _ => Vec::new(),
        };
        // Arithmetic on values the loop does not change is computed before
        // it.
        let invariants = if std::env::var_os("MWCC_IRO_NO_INVARIANT_MOTION").is_none() {
            hoist_invariant_expressions(&mut body[index], function)
        } else {
            Vec::new()
        };
        // (A count register loop's count is set before them.)
        if let Stmt::Counted { count, .. } = &mut body[index] {
            if (!hoisted.is_empty() || std::env::var_os("MWCC_IRO_COUNT_WITH_HOISTS_ONLY").is_none()) && count.as_int().is_some() {
                let ty = count.ty;
                let variable = function.add_temporary(ty);
                hoisted.insert(0, Stmt::Assign { variable, value: count.clone() });
                *count = Expr { kind: ExprKind::Var(variable), ty };
            }
        }
        let (inits, folded) = match &mut body[index] {
            Stmt::Loop { body: inner, step, effects, .. } if !CURSORS.with(|flag| flag.get()) => {
                (reduce_loop_offsets(inner, step, effects, initial, fold_start, function), false)
            }
            Stmt::Counted { body: inner, .. } if !CURSORS.with(|flag| flag.get()) => {
                (reduce_loop_offsets(inner, &mut Vec::new(), &mut [], initial, fold_start, function), false)
            }
            Stmt::Loop { condition, body: inner, step, effects, .. } => {
                reduce_loop(condition.as_mut(), inner, step, effects, initial, fold_start, function)
            }
            Stmt::Counted { guard, body: inner, .. } => {
                reduce_loop(guard.as_mut(), inner, &mut Vec::new(), &mut [], initial, fold_start, function)
            }
            _ => (Vec::new(), false),
        };
        // (And the global addresses it still uses, ahead of those.)
        let addresses = match &body[index] {
            Stmt::Loop { .. } | Stmt::Counted { .. } => hoist_loop_addresses(&mut body[index], function),
            _ => Vec::new(),
        };
        let count = inits.len() + hoisted.len();
        // (Before the induction variable's own initialization when they
        // do not read it; hoisted constants before that.)
        // (A count register loop counting from the variable sets it first.)
        let counts_from_start = matches!(
            (&body[index], initial),
            (Stmt::Counted { count: Expr { kind: ExprKind::Var(counted), .. }, .. }, Some((variable, _))) if *counted == variable
        );
        let before_start = if initial.is_some() && !counts_from_start { index - 1 } else { index };
        let at = if folded { index - 1 } else { index };
        for (offset, init) in inits.into_iter().enumerate() {
            body.insert(at + offset, init);
        }
        // (After the cursors when those precede the start.)
        let before_start = if folded { at + count - hoisted.len() } else { before_start };
        for (offset, constant) in hoisted.into_iter().enumerate() {
            body.insert(before_start + offset, constant);
        }
        // (Right before the loop.)
        let loop_at = index + count;
        let invariants: Vec<Stmt> = addresses.into_iter().chain(invariants).collect();
        let invariant_count = invariants.len();
        for (offset, invariant) in invariants.into_iter().enumerate() {
            body.insert(loop_at + offset, invariant);
        }
        let count = count + invariant_count;
        index += count + 1;
        // (What the statements just passed assign.)
        for statement in &body[index - count - 1..index] {
            match statement {
                Stmt::Assign { variable, value } => {
                    known.retain(|(known, _)| known != variable);
                    if let Some(value) = value.as_int() {
                        known.push((*variable, value));
                    }
                }
                other => {
                    let mut assigned = Vec::new();
                    collect_assigned(std::slice::from_ref(other), &mut assigned);
                    known.retain(|(known, _)| !assigned.contains(known));
                }
            }
        }
    }
}

thread_local! {
    /// Constants known on entry to the statement list being walked.
    static KNOWN: std::cell::RefCell<Vec<(VarId, i64)>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Constants known before the loop being reduced.
    static CONTEXT: std::cell::RefCell<Vec<(VarId, i64)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The induction's known start: the statement before the loop (`true`), or
/// a constant known from enclosing code.
fn known_start(initial: Option<(VarId, i64)>, induction: VarId) -> Option<(i64, bool)> {
    match initial {
        Some((variable, value)) if variable == induction => Some((value, true)),
        _ => CONTEXT.with(|context| {
            context.borrow().iter().find(|(variable, _)| *variable == induction).map(|&(_, value)| (value, false))
        }),
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
    body: &mut Vec<Stmt>,
    step: &mut Vec<Stmt>,
    effects: &mut [Stmt],
    initial: Option<(VarId, i64)>,
    fold_start: bool,
    function: &mut Function,
) -> (Vec<Stmt>, bool) {
    let mut inits = Vec::new();
    let mut folded = true;
    // Induction variables: `v = v + c` in the step (or the body's top
    // level), assigned nowhere else.
    let update = |statement: &Stmt| match statement {
        Stmt::Assign { variable, value } => match &value.kind {
            ExprKind::Binary(BinaryOp::Add, left, right) if left.as_var() == Some(*variable) => {
                right.as_int().map(|increment| (*variable, increment))
            }
            _ => None,
        },
        _ => None,
    };
    let in_body = std::env::var_os("MWCC_IRO_NO_BODY_INDUCTIONS").is_none();
    let inductions: Vec<(bool, usize, VarId, i64)> = step
        .iter()
        .enumerate()
        .filter_map(|(position, statement)| update(statement).map(|(v, c)| (true, position, v, c)))
        .chain(
            body.iter()
                .enumerate()
                .filter(|_| in_body)
                .filter_map(|(position, statement)| update(statement).map(|(v, c)| (false, position, v, c))),
        )
        .collect();
    // Each cursor advances just before its induction variable.
    let (mut step_shift, mut body_shift) = (0, 0);
    for (in_step, position, induction, increment) in inductions {
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
        // MWCC numbers the cursors in reverse evaluation order.
        let mut discovered: Vec<Cursor> = Vec::new();
        let mut scratch = function.clone();
        rewrite_statements(&mut body.clone(), induction, &assigned, &mut discovered, &mut scratch);
        for cursor in discovered.iter().rev() {
            cursor_for(&cursor.base, cursor.stride, &mut cursors, function);
        }
        rewrite_statements(body, induction, &assigned, &mut cursors, function);
        if cursors.is_empty() {
            continue;
        }
        let inits_before = inits.len();
        for cursor in &cursors {
            let ty = cursor.base.ty;
            // (GC/1.0-1.2.5n fold it only while the loop still reads the
            // induction variable's value.)
            let fold = fold_start || uses(body, induction) > usize::from(!in_step);
            let start = match known_start(initial, induction) {
                Some((value, adjacent)) if fold => {
                    folded &= adjacent;
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
            // (Set in evaluation order.)
            inits.insert(inits_before, Stmt::Assign { variable: cursor.variable, value: start });
            let advance = Expr::binary(
                BinaryOp::Add,
                Expr { kind: ExprKind::Var(cursor.variable), ty },
                Expr::int(increment * cursor.stride),
                ty,
            );
            let advance = Stmt::Assign { variable: cursor.variable, value: advance };
            if in_step {
                step.insert(position + step_shift, advance);
                step_shift += 1;
            } else {
                body.insert(position + body_shift, advance);
                body_shift += 1;
            }
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
            Stmt::Counted { body, .. } => count_assignments(body, variable),
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
            Stmt::Counted { body, .. } => collect_assigned(body, out),
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
    // (The backend numbers cursors in creation order.)
    function.variables[variable].name = format!("@cursor{variable}");
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
                rewrite(value, induction, assigned, cursors, function);
                if let Place::Memory { base, index, .. } = place {
                    match index.as_deref().and_then(|index| scaled(index, induction)) {
                        Some(stride) if is_invariant(base, assigned, function) && offset_form(base, stride).is_some() => {
                            let (hoisted, offset) = offset_form(base, stride).expect("checked");
                            **base = hoisted;
                            *index = Some(Box::new(offset));
                        }
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
            Stmt::Counted { guard, body, .. } => {
                if let Some(guard) = guard {
                    rewrite(guard, induction, assigned, cursors, function);
                }
                rewrite_statements(body, induction, assigned, cursors, function);
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
            Some(stride) if is_invariant(base, assigned, function) && offset_form(base, stride).is_some() => {
                let (hoisted, offset) = offset_form(base, stride).expect("checked");
                **base = hoisted;
                *index = Some(Box::new(offset));
            }
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
            if let Some((hoisted, offset)) = offset_form(left, stride) {
                expression.kind = ExprKind::Binary(BinaryOp::Add, Box::new(hoisted), Box::new(offset));
                return;
            }
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
                ExprKind::Idiom(mwcc_iro::Idiom::Update(_, left, right)) => {
                    children.push(left);
                    children.push(right);
                }
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
            ExprKind::Idiom(mwcc_iro::Idiom::Update(_, left, right)) => expression(left, variable) || expression(right, variable),
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
        Stmt::Counted { count, guard, body } => {
            expression(count, variable)
                || guard.as_ref().is_some_and(|guard| expression(guard, variable))
                || based(body, variable)
        }
        Stmt::Switch { value, arms, .. } => expression(value, variable) || arms.iter().any(|arm| based(arm, variable)),
        Stmt::Break | Stmt::Continue | Stmt::Goto(_) | Stmt::Label(_) => false,
    })
}

/// Arithmetic inside a loop on variables it does not assign (no loads, no
/// calls) is computed once before it, into a variable.
fn hoist_invariant_expressions(statement: &mut Stmt, function: &mut Function) -> Vec<Stmt> {
    let mut assigned = Vec::new();
    collect_assigned(std::slice::from_ref(statement), &mut assigned);
    let invariant_variable = |id: VarId, function: &Function| {
        !assigned.contains(&id) && function.variables[id].frame.is_none() && !function.variables[id].volatile
    };
    fn invariant(e: &Expr, check: &dyn Fn(VarId) -> bool) -> bool {
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Float(_) | ExprKind::GlobalAddress(_) | ExprKind::LocalAddress(_) => true,
            ExprKind::Var(id) => check(*id),
            ExprKind::Binary(op, left, right) => {
                !op.is_comparison()
                    && !matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr)
                    && invariant(left, check)
                    && invariant(right, check)
            }
            ExprKind::Unary(_, operand) | ExprKind::Convert(operand) => invariant(operand, check),
            _ => false,
        }
    }
    fn hoistable(e: &Expr) -> bool {
        matches!(e.kind, ExprKind::Binary(..) | ExprKind::Unary(..) | ExprKind::Convert(..))
            && !mwcc_iro::is_wide(e.ty)
            && format!("{:?}", e.kind).contains("Var(")
    }
    struct Hoister<'a> {
        check: &'a dyn Fn(VarId) -> bool,
        found: Vec<(String, Expr, VarId)>,
        function: &'a mut Function,
    }
    impl Hoister<'_> {
        fn expression(&mut self, e: &mut Expr) {
            if hoistable(e) && invariant(e, self.check) {
                let key = format!("{e:?}");
                let variable = match self.found.iter().find(|(k, ..)| *k == key) {
                    Some(&(_, _, variable)) => variable,
                    None => {
                        let variable = self.function.add_temporary(e.ty);
                        self.found.push((key, e.clone(), variable));
                        variable
                    }
                };
                *e = Expr { kind: ExprKind::Var(variable), ty: e.ty };
                return;
            }
            match &mut e.kind {
                // (An address stays in its addressing mode.)
                ExprKind::Load { index, .. } => {
                    if let Some(index) = index {
                        self.expression(index);
                    }
                }
                // (An argument `x + k` is computed into its register as
                // cheaply as it would be copied there.)
                ExprKind::Call { arguments, .. } => {
                    for argument in arguments {
                        let displaced = matches!(&argument.kind, ExprKind::Binary(BinaryOp::Add | BinaryOp::Subtract, left, right)
                            if left.as_var().is_some() && right.as_int().is_some());
                        if !displaced || std::env::var_os("MWCC_IRO_HOIST_DISPLACED_ARGUMENTS").is_some() {
                            self.expression(argument);
                        }
                    }
                }
                _ => crate::passes::children(e, &mut |child| self.expression(child)),
            }
        }
        fn statements(&mut self, body: &mut [Stmt]) {
            for statement in body {
                match statement {
                    Stmt::Assign { value, .. } | Stmt::Eval(value) | Stmt::SetReturn(value) => self.expression(value),
                    Stmt::Return(Some(value)) => self.expression(value),
                    Stmt::Store { place, value, .. } => {
                        if let Place::Memory { index: Some(index), .. } = place {
                            self.expression(index);
                        }
                        self.expression(value);
                    }
                    Stmt::If { condition, then_body, else_body } => {
                        self.expression(condition);
                        self.statements(then_body);
                        self.statements(else_body);
                    }
                    Stmt::Loop { condition, body, step, effects, .. } => {
                        if let Some(condition) = condition {
                            self.expression(condition);
                        }
                        self.statements(body);
                        self.statements(step);
                        self.statements(effects);
                    }
                    Stmt::Counted { body, .. } => self.statements(body),
                    Stmt::Switch { value, arms, .. } => {
                        self.expression(value);
                        arms.iter_mut().for_each(|arm| self.statements(arm));
                    }
                    _ => {}
                }
            }
        }
    }
    let variables: Vec<bool> = (0..function.variables.len()).map(|id| invariant_variable(id, function)).collect();
    let check = |id: VarId| variables.get(id).copied().unwrap_or(false);
    let mut hoister = Hoister { check: &check, found: Vec::new(), function };
    match statement {
        Stmt::Loop { condition, body, step, effects, .. } => {
            if let Some(condition) = condition {
                hoister.expression(condition);
            }
            hoister.statements(body);
            hoister.statements(step);
            hoister.statements(effects);
        }
        Stmt::Counted { body, .. } => hoister.statements(body),
        _ => {}
    }
    hoister
        .found
        .into_iter()
        .map(|(_, value, variable)| Stmt::Assign { variable, value })
        .collect()
}

/// Integer constants stored inside a loop: each becomes a variable set
/// before the loop.
fn hoist_stored_constants(body: &mut [Stmt], function: &mut Function) -> Vec<Stmt> {
    fn visit(body: &mut [Stmt], function: &mut Function, hoisted: &mut Vec<(i64, Type, VarId)>) {
        for statement in body {
            match statement {
                Stmt::Store { value, .. } => {
                    if let Some(constant) = value.as_int() {
                        if !mwcc_iro::is_float(value.ty) {
                            // (A narrow constant in its promoted form.)
                            let ty = mwcc_iro::promote(value.ty);
                            let constant = if mwcc_iro::is_unsigned_narrow(value.ty) {
                                constant & ((1i64 << mwcc_iro::width(value.ty)) - 1)
                            } else {
                                constant
                            };
                            let variable = match hoisted.iter().find(|(k, t, _)| *k == constant && *t == ty) {
                                Some(&(_, _, variable)) => variable,
                                None => {
                                    let variable = function.add_temporary(ty);
                                    function.variables[variable].name = format!("@hoist{variable}");
                                    hoisted.push((constant, ty, variable));
                                    variable
                                }
                            };
                            *value = Expr { kind: ExprKind::Var(variable), ty };
                        }
                    }
                }
                Stmt::If { then_body, else_body, .. } => {
                    visit(then_body, function, hoisted);
                    visit(else_body, function, hoisted);
                }
                Stmt::Loop { body, step, effects, .. } => {
                    visit(body, function, hoisted);
                    visit(step, function, hoisted);
                    visit(effects, function, hoisted);
                }
                Stmt::Counted { body, .. } => visit(body, function, hoisted),
                Stmt::Switch { arms, .. } => arms.iter_mut().for_each(|arm| visit(arm, function, hoisted)),
                _ => {}
            }
        }
    }
    let mut hoisted = Vec::new();
    visit(body, function, &mut hoisted);
    hoisted
        .into_iter()
        .map(|(constant, ty, variable)| Stmt::Assign { variable, value: Expr { kind: ExprKind::Int(constant), ty } })
        .collect()
}

/// Induction variables read only by their own updates go (after strength
/// reduction moved their other reads to cursors).
pub fn remove_dead_inductions(function: &mut Function) {
    fn self_updates(body: &[Stmt], variable: VarId) -> usize {
        body.iter()
            .map(|statement| match statement {
                Stmt::Assign { variable: assigned, value } if *assigned == variable => usize::from(value.mentions(variable)),
                Stmt::If { then_body, else_body, .. } => self_updates(then_body, variable) + self_updates(else_body, variable),
                Stmt::Loop { body, step, effects, .. } => {
                    self_updates(body, variable) + self_updates(step, variable) + self_updates(effects, variable)
                }
                Stmt::Counted { body, .. } => self_updates(body, variable),
                Stmt::Switch { arms, .. } => arms.iter().map(|arm| self_updates(arm, variable)).sum(),
                _ => 0,
            })
            .sum()
    }
    fn remove(body: &mut Vec<Stmt>, variable: VarId) {
        body.retain(|statement| !matches!(statement, Stmt::Assign { variable: assigned, value } if *assigned == variable && !format!("{value:?}").contains("Call {")));
        for statement in body.iter_mut() {
            match statement {
                Stmt::If { then_body, else_body, .. } => {
                    remove(then_body, variable);
                    remove(else_body, variable);
                }
                Stmt::Loop { body, step, effects, .. } => {
                    remove(body, variable);
                    remove(step, variable);
                    remove(effects, variable);
                }
                Stmt::Counted { body, .. } => remove(body, variable),
                Stmt::Switch { arms, .. } => arms.iter_mut().for_each(|arm| remove(arm, variable)),
                _ => {}
            }
        }
    }
    for variable in 0..function.variables.len() {
        let local = &function.variables[variable];
        if local.kind == VariableKind::Parameter || local.frame.is_some() || local.volatile {
            continue;
        }
        let updates = self_updates(&function.body, variable);
        if updates > 0 && uses(&function.body, variable) == updates {
            remove(&mut function.body, variable);
        }
    }
}

/// Strength reduction without an explicit speed goal: an address
/// `base + i*k` reads a hoisted base indexed by an offset variable stepped
/// with `i` (one per stride).
fn reduce_loop_offsets(
    body: &mut Vec<Stmt>,
    step: &mut Vec<Stmt>,
    effects: &mut [Stmt],
    initial: Option<(VarId, i64)>,
    fold_start: bool,
    function: &mut Function,
) -> Vec<Stmt> {
    let update = |statement: &Stmt| match statement {
        Stmt::Assign { variable, value } => match &value.kind {
            ExprKind::Binary(BinaryOp::Add, left, right) if left.as_var() == Some(*variable) => {
                right.as_int().map(|increment| (*variable, increment))
            }
            _ => None,
        },
        _ => None,
    };
    let inductions: Vec<(bool, usize, VarId, i64)> = step
        .iter()
        .enumerate()
        .filter_map(|(position, statement)| update(statement).map(|(v, c)| (true, position, v, c)))
        .chain(
            body.iter()
                .enumerate()
                .filter_map(|(position, statement)| update(statement).map(|(v, c)| (false, position, v, c))),
        )
        .collect();
    let mut inits = Vec::new();
    let (mut step_shift, mut body_shift) = (0, 0);
    for (in_step, position, induction, increment) in inductions {
        let variable = &function.variables[induction];
        if variable.frame.is_some() || variable.volatile || !mwcc_iro::is_general_word(variable.ty) {
            continue;
        }
        if count_assignments(body, induction) + count_assignments(step, induction) + count_assignments(effects, induction) != 1 {
            continue;
        }
        let mut assigned = Vec::new();
        collect_assigned(body, &mut assigned);
        collect_assigned(step, &mut assigned);
        collect_assigned(effects, &mut assigned);
        // Discover (base, stride) pairs in evaluation order.
        let mut discovered: Vec<Cursor> = Vec::new();
        let mut scratch = function.clone();
        rewrite_statements(&mut body.clone(), induction, &assigned, &mut discovered, &mut scratch);
        if discovered.is_empty() {
            continue;
        }
        // Hoisted bases (a register variable serves as its own), then one
        // offset per stride.
        let mut bases: Vec<(String, Expr)> = Vec::new();
        for cursor in &discovered {
            if bases.iter().any(|(key, _)| *key == cursor.key) {
                continue;
            }
            let base = match cursor.base.kind {
                ExprKind::Var(_) => cursor.base.clone(),
                _ => {
                    let variable = function.add_temporary(cursor.base.ty);
                    function.variables[variable].name = format!("@cursor{variable}");
                    inits.push(Stmt::Assign { variable, value: cursor.base.clone() });
                    Expr { kind: ExprKind::Var(variable), ty: cursor.base.ty }
                }
            };
            bases.push((cursor.key.clone(), base));
        }
        let mut offsets: Vec<(i64, VarId)> = Vec::new();
        for cursor in &discovered {
            if offsets.iter().any(|(stride, _)| *stride == cursor.stride) {
                continue;
            }
            let variable = function.add_temporary(mwcc_iro::Type::Int);
            function.variables[variable].name = format!("@cursor{variable}");
            let start = match known_start(initial, induction) {
                Some((value, _)) if fold_start => Expr::int(value * cursor.stride),
                _ => Expr::binary(
                    BinaryOp::Multiply,
                    Expr { kind: ExprKind::Var(induction), ty: function.variables[induction].ty },
                    Expr::int(cursor.stride),
                    mwcc_iro::Type::Int,
                ),
            };
            inits.push(Stmt::Assign { variable, value: start });
            offsets.push((cursor.stride, variable));
            let advance = Stmt::Assign {
                variable,
                value: Expr::binary(
                    BinaryOp::Add,
                    Expr { kind: ExprKind::Var(variable), ty: mwcc_iro::Type::Int },
                    Expr::int(increment * cursor.stride),
                    mwcc_iro::Type::Int,
                ),
            };
            // (Each offset advances after its induction variable.)
            let at = position + offsets.len();
            if in_step {
                step.insert(at + step_shift, advance);
            } else {
                body.insert(at + body_shift, advance);
            }
        }
        if in_step {
            step_shift += offsets.len();
        } else {
            body_shift += offsets.len();
        }
        OFFSETS.with(|table| {
            *table.borrow_mut() = discovered
                .iter()
                .map(|cursor| {
                    let base = bases.iter().find(|(key, _)| *key == cursor.key).expect("hoisted").1.clone();
                    let offset = offsets.iter().find(|(stride, _)| *stride == cursor.stride).expect("made").1;
                    (cursor.key.clone(), cursor.stride, base, offset)
                })
                .collect();
        });
        let mut cursors = Vec::new();
        rewrite_statements(body, induction, &assigned, &mut cursors, function);
        OFFSETS.with(|table| table.borrow_mut().clear());
    }
    inits
}

thread_local! {
    /// While set: (base key, stride) → (base, offset variable): addresses
    /// rewrite to `base + offset` instead of a cursor.
    static OFFSETS: std::cell::RefCell<Vec<(String, i64, Expr, VarId)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The offset form of `base + i*stride`, while one is being made.
fn offset_form(base: &Expr, stride: i64) -> Option<(Expr, Expr)> {
    let key = format!("{:?}", base);
    OFFSETS.with(|table| {
        table.borrow().iter().find(|(k, s, ..)| *k == key && *s == stride).map(|(_, _, base, offset)| {
            (base.clone(), Expr { kind: ExprKind::Var(*offset), ty: mwcc_iro::Type::Int })
        })
    })
}

/// An induction update in a count-register loop that nothing after reads
/// (not the loop, not the code after it) goes.
pub fn remove_dead_counted_updates(function: &mut Function) {
    fn walk(body: &mut [Stmt], read_later: &dyn Fn(VarId) -> bool, in_loop: bool) {
        for index in 0..body.len() {
            let (before, after) = body.split_at_mut(index + 1);
            let statement = &mut before[index];
            let later = |variable: VarId| uses(after, variable) > 0 || read_later(variable);
            match statement {
                Stmt::If { then_body, else_body, .. } => {
                    walk(then_body, &later, in_loop);
                    walk(else_body, &later, in_loop);
                }
                Stmt::Counted { body: inner, .. } if !in_loop => {
                    let dead: Vec<VarId> = inner
                        .iter()
                        .filter_map(|statement| match statement {
                            Stmt::Assign { variable, value } if value.mentions(*variable) => Some(*variable),
                            _ => None,
                        })
                        .filter(|&variable| {
                            uses(inner, variable) == 1 && count_assignments(inner, variable) == 1 && !later(variable)
                        })
                        .collect();
                    inner.retain(|statement| !matches!(statement, Stmt::Assign { variable, .. } if dead.contains(variable)));
                }
                _ => {}
            }
        }
    }
    let mut body = std::mem::take(&mut function.body);
    walk(&mut body, &|_| false, false);
    function.body = body;
}

thread_local! {
    /// The globals whose addresses loop code motion takes out of a loop
    /// (absolute: neither small data nor functions), and whether a bare
    /// address value goes too (GC/1.x-2.x; GC/3.x and Wii keep it).
    static LOOP_ADDRESSES: std::cell::RefCell<(std::collections::HashSet<String>, bool)> =
        std::cell::RefCell::new((std::collections::HashSet::new(), false));
    static LOOP_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Set the globals [`hoist_loop_addresses`] takes out of loops.
pub fn set_loop_addresses(names: std::collections::HashSet<String>, values: bool) {
    LOOP_ADDRESSES.with(|cell| *cell.borrow_mut() = (names, values));
}

/// A global's address used in an outermost loop is computed once before it
/// (`&g`, and `&g + k` before GC/3.x, as a memory base or a value).
fn hoist_loop_addresses(statement: &mut Stmt, function: &mut Function) -> Vec<Stmt> {
    if LOOP_DEPTH.with(|depth| depth.get()) > 0 || std::env::var_os("MWCC_IRO_NO_LOOP_ADDRESSES").is_some() {
        return Vec::new();
    }
    struct Hoister<'a> {
        names: &'a std::collections::HashSet<String>,
        values: bool,
        found: Vec<(String, Expr, VarId)>,
        function: &'a mut Function,
    }
    impl Hoister<'_> {
        fn replace(&mut self, e: &mut Expr) {
            let key = format!("{e:?}");
            let variable = match self.found.iter().find(|(k, ..)| *k == key) {
                Some(&(_, _, variable)) => variable,
                None => {
                    let variable = self.function.add_temporary(e.ty);
                    self.found.push((key, e.clone(), variable));
                    variable
                }
            };
            *e = Expr { kind: ExprKind::Var(variable), ty: e.ty };
        }
        fn hoisted(&self, e: &Expr) -> bool {
            matches!(&e.kind, ExprKind::GlobalAddress(name) if self.names.contains(name))
        }
        fn expression(&mut self, e: &mut Expr, base: bool) {
            if self.hoisted(e) {
                if base || self.values {
                    self.replace(e);
                }
                return;
            }
            match &mut e.kind {
                ExprKind::Binary(BinaryOp::Add, left, right) if self.hoisted(left) && right.as_int().is_some() => {
                    if self.values {
                        self.replace(e);
                    } else {
                        self.replace(left);
                    }
                }
                ExprKind::Load { base, index, .. } => {
                    self.expression(base, true);
                    if let Some(index) = index {
                        self.expression(index, false);
                    }
                }
                _ => crate::passes::children(e, &mut |child| self.expression(child, false)),
            }
        }
        fn statements(&mut self, body: &mut [Stmt]) {
            for statement in body {
                match statement {
                    Stmt::Assign { value, .. } | Stmt::Eval(value) | Stmt::SetReturn(value) => self.expression(value, false),
                    Stmt::Return(Some(value)) => self.expression(value, false),
                    Stmt::Store { place, value, .. } => {
                        if let Place::Memory { base, index, .. } = place {
                            self.expression(base, true);
                            if let Some(index) = index {
                                self.expression(index, false);
                            }
                        }
                        self.expression(value, false);
                    }
                    Stmt::If { condition, then_body, else_body } => {
                        self.expression(condition, false);
                        self.statements(then_body);
                        self.statements(else_body);
                    }
                    Stmt::Loop { condition, body, step, effects, .. } => {
                        if let Some(condition) = condition {
                            self.expression(condition, false);
                        }
                        self.statements(body);
                        self.statements(step);
                        self.statements(effects);
                    }
                    Stmt::Counted { body, .. } => self.statements(body),
                    Stmt::Switch { value, arms, .. } => {
                        self.expression(value, false);
                        arms.iter_mut().for_each(|arm| self.statements(arm));
                    }
                    _ => {}
                }
            }
        }
    }
    LOOP_ADDRESSES.with(|cell| {
        let (names, values) = &*cell.borrow();
        if names.is_empty() {
            return Vec::new();
        }
        let mut hoister = Hoister { names, values: *values, found: Vec::new(), function };
        hoister.statements(std::slice::from_mut(statement));
        hoister.found.into_iter().map(|(_, value, variable)| Stmt::Assign { variable, value }).collect()
    })
}
