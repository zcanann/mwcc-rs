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
        .filter(|&id| {
            let variable = &function.variables[id];
            variable.frame.is_some() && variable.kind == VariableKind::Local && !variable.volatile
        })
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
                initialized: false,
            raw: false,
            volatile: false,
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
            // A whole-struct copy reads the object as memory.
            ExprKind::Load { base, .. } if matches!(e.ty, Type::Struct { .. }) && base.mentions(id) => *escapes = true,
            ExprKind::Load { base, index: None, offset } if is_address_of(base, id) => fields.push((*offset, e.ty)),
            ExprKind::LocalAddress(x) if *x == id => *escapes = true,
            _ => children(e, &mut |child| expression(child, id, fields, escapes)),
        }
    }
    for statement in body {
        match statement {
            Stmt::Store { place: Place::Memory { base, .. }, ty: Type::Struct { .. }, value, .. } if base.mentions(id) => {
                *escapes = true;
                expression(value, id, fields, escapes);
            }
            Stmt::Store { place: Place::Memory { base, index: None, offset }, ty, value, .. } if is_address_of(base, id) => {
                fields.push((*offset, *ty));
                expression(value, id, fields, escapes);
            }
            Stmt::If { condition, then_body, else_body } => {
                expression(condition, id, fields, escapes);
                scan_fields(then_body, id, fields, escapes);
                scan_fields(else_body, id, fields, escapes);
            }
            Stmt::Loop { condition, body, step, effects, .. } => {
                if let Some(condition) = condition {
                    expression(condition, id, fields, escapes);
                }
                scan_fields(body, id, fields, escapes);
                scan_fields(step, id, fields, escapes);
                scan_fields(effects, id, fields, escapes);
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
            Stmt::Store { place: Place::Memory { base, index: None, offset }, ty, mut value, .. } if is_address_of(&base, id) => {
                expression(&mut value, id, map);
                let variable = map[&offset];
                out.push(Stmt::Assign { variable, value });
                if keep {
                    out.push(Stmt::Store {
                        place: Place::Memory { base, index: None, offset },
                        ty,
                        value: Expr { kind: ExprKind::Var(variable), ty },
                        compound: false,
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
            Stmt::Loop { test_first, mut condition, body, step, effects } => {
                if let Some(condition) = &mut condition {
                    expression(condition, id, map);
                }
                out.push(Stmt::Loop {
                    test_first,
                    condition,
                    body: replace_fields(body, id, map, keep),
                    step: replace_fields(step, id, map, keep),
                    effects: replace_fields(effects, id, map, keep),
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

/// Run every pass in order. Branch-preserving builds (GC/1.0-1.2.5n) form
/// no sign-mask idioms and keep two-way assignments as branches.
pub fn run(function: &mut Function, branch_preserving: bool, reassociates_sums: bool, unrolling: bool) {
    let enabled = |name: &str| std::env::var_os(format!("MWCC_IRO_NO_{name}")).is_none();
    if enabled("UNROLL") && !branch_preserving && unrolling {
        unroll(&mut function.body);
        merge_constant_updates(&mut function.body);
    }
    if enabled("FOLD") {
        for_each_expression(&mut function.body, &mut |expression| fold(expression));
    }
    if enabled("CONSTANT_BRANCHES") {
        for_each_expression(&mut function.body, &mut |expression| logical_constants(expression));
        constant_branches(&mut function.body);
    }
    if enabled("FORWARD_UPDATES") && !has_label(&function.body) {
        forward_updates(function);
    }
    if std::env::var_os("MWCC_IRO_NO_SINGLE_USES").is_none() && !has_label(&function.body) {
        forward_single_uses(function);
    }
    // (Forwarding reasons in structured order: not across `goto`s.)
    if enabled("FORWARD") && !has_label(&function.body) {
        forward_offsets(function);
    }
    if enabled("ALGEBRA") {
        for_each_expression(&mut function.body, &mut |expression| algebra(expression));
    }
    if enabled("REASSOCIATE") && reassociates_sums {
        fn all(expression: &mut Expr) {
            children(expression, &mut |child| all(child));
            reassociate_sum(expression);
        }
        for_each_expression(&mut function.body, &mut |expression| all(expression));
        // (Reassociation can pair literals.)
        if enabled("FOLD") {
            for_each_expression(&mut function.body, &mut |expression| fold(expression));
        }
    }
    if enabled("HOIST_CONSTANTS") {
        for_each_expression(&mut function.body, &mut |expression| hoist_constants(expression, reassociates_sums));
    }
    if enabled("DISPLACEMENTS") {
        displacements(&mut function.body);
    }
    if enabled("MEMBER_INDEX_DISPLACEMENTS") {
        member_index_displacements(&mut function.body);
    }
    if enabled("IDIOMS") && !branch_preserving {
        let variables: Vec<Type> = function.variables.iter().map(|variable| variable.ty).collect();
        for_each_expression(&mut function.body, &mut |expression| idioms(expression, &variables));
    }
    if enabled("SELECTS") && !branch_preserving {
        selects(function);
    }
    if enabled("STORES") {
        stores(&mut function.body, true);
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
        // (Only integer low-bit arithmetic: a floating operation needs its
        // converted operands.)
        if mwcc_iro::is_float(expression.ty) {
            return;
        }
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
    /// `x & m` with `m` within the low N bytes (`char a & 0xf`): x's
    /// promotion from a narrow type at least N bytes wide is dropped.
    fn mask_narrowing(value: &mut Expr) {
        // (Also the operands of a comparison that is the value.)
        if let ExprKind::Binary(op, left, right) = &mut value.kind {
            if op.is_comparison() {
                mask_narrowing(left);
                mask_narrowing(right);
                return;
            }
        }
        let ExprKind::Binary(BinaryOp::BitAnd, left, right) = &mut value.kind else { return };
        let mask = right.as_int().filter(|&mask| mask > 0 && mask <= 0xffff);
        let Some(mask) = mask.filter(|_| std::env::var_os("MWCC_IRO_NO_MASK_NARROWING").is_none()) else { return };
        // (Through a right shift, the bits the mask keeps move up.)
        let (operand, mask) = match &mut left.kind {
            ExprKind::Binary(BinaryOp::ShiftRight, inner, count) if count.as_int().is_some_and(|n| (0..16).contains(&n)) => {
                let n = count.as_int().unwrap_or(0);
                (inner.as_mut(), mask << n)
            }
            _ => (left.as_mut(), mask),
        };
        if mask <= 0xffff {
            strip(operand, if mask <= 0xff { 1 } else { 2 });
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
            Stmt::Loop { body, step, effects, .. } => {
                narrowing_in(body, return_type, variables);
                narrowing_in(step, return_type, variables);
                narrowing_in(effects, return_type, variables);
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
            // (Only a statement's own value: MWCC extends a masked narrow
            // value inside a condition.)
            mask_narrowing(value);
            if is_narrow(ty) {
                strip(value, width(ty));
                // (A literal narrowed on the way out is truncated.)
                if let Some(literal) = value.as_int().filter(|_| std::env::var_os("MWCC_IRO_NO_NARROW_LITERALS").is_none()) {
                    let truncated = match ty {
                        Type::Char => i64::from(literal as i8),
                        Type::UnsignedChar => i64::from(literal as u8),
                        Type::Short => i64::from(literal as i16),
                        _ => i64::from(literal as u16),
                    };
                    *value = Expr::typed_int(truncated, value.ty);
                }
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
            // (A null pointer cast stays a value: `p != (void *)0`
            // materializes the zero.)
            ExprKind::Convert(operand)
                if matches!(expression.ty, Type::Int | Type::UnsignedInt)
                    || (matches!(expression.ty, Type::Pointer(_) | Type::StructPointer { .. })
                        && (operand.as_int() != Some(0) || std::env::var_os("MWCC_IRO_O0_FOLD_POINTER_CASTS").is_some())) =>
            {
                operand.as_int().map(|value| Expr::typed_int(value, expression.ty))
            }
            // An integer constant converted to a floating type is that
            // floating constant.
            ExprKind::Convert(operand)
                if mwcc_iro::is_float(expression.ty)
                    && operand.as_int().is_some()
                    && std::env::var_os("MWCC_IRO_O0_RUNTIME_CONSTANT_CONVERSION").is_none() =>
            {
                let value = operand.as_int().expect("checked");
                let value = if mwcc_iro::is_unsigned(operand.ty) { f64::from(value as u32) } else { value as f64 };
                Some(Expr { kind: ExprKind::Float(value), ty: expression.ty })
            }
            ExprKind::Binary(op, left, right) => match (left.as_int(), right.as_int()) {
                (Some(_), Some(_)) if mwcc_iro::is_wide(expression.ty) || mwcc_iro::is_wide(left.ty) || mwcc_iro::is_wide(right.ty) => {
                    fold_wide(*op, left, right, expression.ty).map(|value| Expr::typed_int(value, expression.ty))
                }
                (Some(_), Some(_)) => fold_typed(*op, left, right).map(|value| Expr::typed_int(value, expression.ty)),
                _ => None,
            },
            _ => None,
        };
        if let Some(folded) = folded {
            *expression = folded;
        }
    }
    for_each_expression(&mut function.body, &mut |expression| literals(expression));
    if std::env::var_os("MWCC_IRO_NO_CONSTANT_BRANCHES").is_none() {
        for_each_expression(&mut function.body, &mut |expression| logical_constants(expression));
        constant_branches(&mut function.body);
    }
    stores(&mut function.body, false);
    if std::env::var_os("MWCC_IRO_NO_REASSOCIATE_OFFSETS").is_none() {
        for_each_expression(&mut function.body, &mut |expression| reassociate_offsets(expression));
    }
    displacements_with(&mut function.body, false);
    narrowing(function);
}

/// `(p + x) + k` is `p + (x + k)`: a constant offset rides the index
/// (`addi i,i,k; add p,i`; a load's displacement takes it back).
fn reassociate_offsets(expression: &mut Expr) {
    children(expression, &mut |child| reassociate_offsets(child));
    let pointer = |ty: Type| matches!(ty, Type::Pointer(_) | Type::StructPointer { .. });
    if !pointer(expression.ty) {
        return;
    }
    let ExprKind::Binary(BinaryOp::Add, left, constant) = &expression.kind else { return };
    if constant.as_int().is_none() {
        return;
    }
    let ExprKind::Binary(BinaryOp::Add, base, index) = &left.kind else { return };
    if !pointer(base.ty) || pointer(index.ty) || index.as_int().is_some() || base.as_int().is_some() {
        return;
    }
    let index_ty = index.ty;
    let shifted = Expr::binary(BinaryOp::Add, index.as_ref().clone(), constant.as_ref().clone(), index_ty);
    *expression = Expr::binary(BinaryOp::Add, base.as_ref().clone(), shifted, expression.ty);
}

/// `x && 0`, `x || 1` (x without effects) and a constant left operand
/// decide a logical operator.
fn logical_constants(expression: &mut Expr) {
    children(expression, &mut |child| logical_constants(child));
    let ExprKind::Binary(op @ (BinaryOp::LogicalAnd | BinaryOp::LogicalOr), left, right) = &expression.kind else { return };
    let and = *op == BinaryOp::LogicalAnd;
    let decided = |value: i64| (value != 0) != and;
    let folded = match (left.as_int(), right.as_int()) {
        (Some(value), _) if decided(value) => Some(i64::from(!and)),
        (_, Some(value)) if decided(value) && speculable(left) => Some(i64::from(!and)),
        _ => None,
    };
    if let Some(value) = folded {
        *expression = Expr::typed_int(value, expression.ty);
    }
}

// ---------------------------------------------------------------- unrolling

/// The unroller's full expansion: a counted loop `v = a; while (v < b) {
/// body; v = v + 1; }` with constant bounds, a straight-line call-free body
/// that leaves `v` alone, a trip count whose prime factors are all at most 7
/// and at most 48 operations in all becomes that many copies of its body
/// (with `v` the iteration's constant), then `v = b`. Other counted loops
/// MWCC unrolls partially (not modeled: the lowering refuses them).
pub fn unroll(body: &mut Vec<Stmt>) {
    for statement in body.iter_mut() {
        match statement {
            Stmt::If { then_body, else_body, .. } => {
                unroll(then_body);
                unroll(else_body);
            }
            Stmt::Loop { body, .. } => unroll(body),
            Stmt::Switch { arms, .. } => arms.iter_mut().for_each(unroll),
            _ => {}
        }
    }
    let mut index = 1;
    while index < body.len() {
        let Some((_, statements)) = unrolled(&body[index - 1], &body[index]) else {
            index += 1;
            continue;
        };
        // (The start assignment goes too: the copies hold the constants
        // and the final value follows them.)
        let count = statements.len();
        body.splice(index - 1..=index, statements);
        index += count - 1;
    }
}

fn unrolled(before: &Stmt, statement: &Stmt) -> Option<(VarId, Vec<Stmt>)> {
    let Stmt::Assign { variable, value: start } = before else { return None };
    let start = start.as_int()?;
    let Stmt::Loop { test_first: true, condition: Some(condition), body, step, effects } = statement else { return None };
    if !effects.is_empty() {
        return None;
    }
    let variable = *variable;
    // `v = v + 1`.
    let [Stmt::Assign { variable: stepped, value: increment }] = step.as_slice() else { return None };
    if *stepped != variable {
        return None;
    }
    let ExprKind::Binary(BinaryOp::Add, base, one) = &increment.kind else { return None };
    if base.as_var() != Some(variable) || one.as_int() != Some(1) {
        return None;
    }
    // `v < n` / `v <= n`.
    let ExprKind::Binary(op, left, right) = &condition.kind else { return None };
    let (Some(tested), Some(bound)) = (left.as_var(), right.as_int()) else { return None };
    if tested != variable {
        return None;
    }
    let end = match op {
        BinaryOp::Less => bound,
        BinaryOp::LessEqual => bound + 1,
        _ => return None,
    };
    let trips = end - start;
    if trips < 1 {
        return None;
    }
    let mut remaining = trips;
    for prime in [2, 3, 5, 7] {
        while remaining % prime == 0 {
            remaining /= prime;
        }
    }
    if remaining != 1 {
        return None;
    }
    // A straight-line, call-free body that leaves the induction alone.
    fn plain(statement: &Stmt, variable: VarId) -> bool {
        let calls = format!("{statement:?}").contains("Call {");
        !calls
            && match statement {
                Stmt::Assign { variable: assigned, .. } => *assigned != variable,
                Stmt::Store { .. } => true,
                _ => false,
            }
    }
    if body.is_empty() || !body.iter().all(|statement| plain(statement, variable)) {
        return None;
    }
    fn operations(e: &Expr) -> i64 {
        match &e.kind {
            ExprKind::Load { .. } => 1,
            ExprKind::Binary(_, left, right) => 1 + operations(left) + operations(right),
            ExprKind::Unary(_, operand) | ExprKind::Convert(operand) => 1 + operations(operand),
            _ => 0,
        }
    }
    let cost: i64 = body
        .iter()
        .map(|statement| match statement {
            Stmt::Assign { value, .. } | Stmt::Store { value, .. } => 1 + operations(value),
            _ => 1,
        })
        .sum();
    if cost * trips > 48 {
        return None;
    }
    let mut out = Vec::new();
    for k in 0..trips {
        let mut copy = body.clone();
        substitute(&mut copy, variable, &Expr::int(start + k));
        out.extend(copy);
    }
    out.push(Stmt::Assign { variable, value: Expr::int(end) });
    Some((variable, out))
}

/// `v = k; v = f(v);` (an unrolled accumulation) is `v = f(k);`, folded.
fn merge_constant_updates(body: &mut Vec<Stmt>) {
    let mut index = 0;
    while index + 1 < body.len() {
        let merged = match (&body[index], &body[index + 1]) {
            (Stmt::Assign { variable, value: constant }, Stmt::Assign { variable: next, value })
                if variable == next && constant.as_int().is_some() && value.mentions(*variable) && !format!("{value:?}").contains("Call {") =>
            {
                let mut value = value.clone();
                let mut statement = [Stmt::Eval(value.clone())];
                substitute(&mut statement, *variable, constant);
                if let [Stmt::Eval(substituted)] = statement {
                    value = substituted;
                }
                algebra(&mut value);
                fold(&mut value);
                Some(Stmt::Assign { variable: *variable, value })
            }
            _ => None,
        };
        match merged {
            Some(statement) => {
                body[index] = statement;
                body.remove(index + 1);
            }
            None => index += 1,
        }
    }
}

/// Whether statements hold a source label (a `goto` target).
pub fn has_label(body: &[Stmt]) -> bool {
    body.iter().any(|statement| match statement {
        Stmt::Label(_) => true,
        Stmt::If { then_body, else_body, .. } => has_label(then_body) || has_label(else_body),
        Stmt::Loop { body, step, effects, .. } => has_label(body) || has_label(step) || has_label(effects),
        Stmt::Switch { arms, .. } => arms.iter().any(|arm| has_label(arm)),
        _ => false,
    })
}

/// Branches on constants (even at -O0): the taken arm replaces an `if`, a
/// loop never entered disappears, and a `do ... while (0)` without
/// `break`/`continue` becomes its body.
pub fn constant_branches(body: &mut Vec<Stmt>) {
    fn loop_control(body: &[Stmt]) -> bool {
        body.iter().any(|statement| match statement {
            Stmt::Break | Stmt::Continue => true,
            Stmt::If { then_body, else_body, .. } => loop_control(then_body) || loop_control(else_body),
            Stmt::Switch { arms, .. } => arms.iter().any(|arm| arm.iter().any(|s| matches!(s, Stmt::Continue))),
            _ => false,
        })
    }
    let mut out = Vec::with_capacity(body.len());
    for mut statement in std::mem::take(body) {
        match &mut statement {
            Stmt::If { then_body, else_body, .. } => {
                constant_branches(then_body);
                constant_branches(else_body);
            }
            Stmt::Loop { body, step, effects, .. } => {
                constant_branches(body);
                constant_branches(step);
                constant_branches(effects);
            }
            Stmt::Switch { arms, .. } => arms.iter_mut().for_each(constant_branches),
            _ => {}
        }
        match statement {
            // (Code holding a `goto` target is never dropped.)
            Stmt::If { condition, then_body, else_body }
                if condition.as_int().is_some()
                    && !has_label(if condition.as_int() != Some(0) { &else_body } else { &then_body }) =>
            {
                out.extend(if condition.as_int() != Some(0) { then_body } else { else_body });
            }
            // (A false first test still runs the effects before it.)
            Stmt::Loop { test_first: true, condition: Some(condition), effects, body, .. }
                if condition.as_int() == Some(0) && !has_label(&body) =>
            {
                out.extend(effects);
            }
            Stmt::Loop { test_first: false, condition: Some(condition), body, step, effects }
                if condition.as_int() == Some(0) && !loop_control(&body) =>
            {
                out.extend(body);
                out.extend(step);
                out.extend(effects);
            }
            // A switch on a constant runs the selected arm (falling through
            // to the next) up to its `break`.
            Stmt::Switch { value, cases, arms, default }
                if value.as_int().is_some()
                    && !arms.iter().any(|arm| has_label(arm))
                    && std::env::var_os("MWCC_IRO_NO_CONSTANT_SWITCH").is_none() =>
            {
                let selected = value.as_int().expect("checked");
                let start = cases.iter().find(|&&(case, _)| case == selected).map(|&(_, arm)| arm).or(default);
                let Some(start) = start else { continue };
                let mut taken = Vec::new();
                let mut ended = false;
                for arm in &arms[start..] {
                    for statement in arm {
                        if matches!(statement, Stmt::Break) {
                            ended = true;
                            break;
                        }
                        taken.push(statement.clone());
                    }
                    if ended {
                        break;
                    }
                }
                // (A `break` nested in the taken code would leave the
                // switch: keep the switch then.)
                fn nested_break(body: &[Stmt]) -> bool {
                    body.iter().any(|statement| match statement {
                        Stmt::Break => true,
                        Stmt::If { then_body, else_body, .. } => nested_break(then_body) || nested_break(else_body),
                        _ => false,
                    })
                }
                if nested_break(&taken) {
                    out.push(Stmt::Switch { value, cases, arms, default });
                } else {
                    out.extend(taken);
                }
            }
            other => out.push(other),
        }
    }
    *body = out;
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
        // A constant index (an unrolled `p[k]`) is a displacement.
        if let Some(constant) = index.as_deref().and_then(Expr::as_int) {
            if let Ok(total) = i16::try_from(i64::from(*offset) + constant) {
                *index = None;
                *offset = i32::from(total);
            }
        }
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
            let Ok(total) = i16::try_from(i64::from(*offset) + value) else {
                // (A large one: its high half stays on the base, `addis`,
                // and its low half joins the displacement.)
                let total = i64::from(*offset) + value;
                let low = ((total + 0x8000) & 0xffff) - 0x8000;
                let high = total - low;
                if distribute
                    && high != value
                    && i32::try_from(total).is_ok()
                    && std::env::var_os("MWCC_IRO_NO_SPLIT_DISPLACEMENTS").is_none()
                {
                    let ty = base.ty;
                    *base = Box::new(Expr::binary(BinaryOp::Add, (**inner).clone(), Expr::int(high), ty));
                    *offset = low as i32;
                }
                break;
            };
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
            Stmt::Loop { body, step, effects, .. } => {
                displacements_with(body, distribute);
                displacements_with(step, distribute);
                displacements_with(effects, distribute);
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
            Stmt::Loop { condition, body, step, effects, .. } => {
                if let Some(condition) = condition {
                    rewrite(condition);
                }
                for_each_expression(body, rewrite);
                for_each_expression(step, rewrite);
                for_each_expression(effects, rewrite);
            }
            Stmt::Counted { count, guard, body } => {
                rewrite(count);
                if let Some(guard) = guard {
                    rewrite(guard);
                }
                for_each_expression(body, rewrite);
            }
            Stmt::Switch { value, arms, .. } => {
                rewrite(value);
                for arm in arms {
                    for_each_expression(arm, rewrite);
                }
            }
            Stmt::Break | Stmt::Continue | Stmt::Goto(_) | Stmt::Label(_) => {}
        }
    }
}

/// GC/1.0-1.2.5n index an absolute array by adding the index to its
/// address and accessing at a displacement (`add; lwz 0(r)`), never `lwzx`.
pub fn unindexed_absolute(body: &mut [Stmt], absolute: &dyn Fn(&str) -> bool) {
    let sum = |base: &mut Box<Expr>, index: &mut Option<Box<Expr>>| {
        if matches!(&base.kind, ExprKind::GlobalAddress(name) if absolute(name)) {
            if let Some(index) = index.take() {
                let ty = base.ty;
                **base = Expr::binary(BinaryOp::Add, (**base).clone(), *index, ty);
            }
        }
    };
    fn expression(e: &mut Expr, sum: &dyn Fn(&mut Box<Expr>, &mut Option<Box<Expr>>)) {
        children(e, &mut |child| expression(child, sum));
        if let ExprKind::Load { base, index, .. } = &mut e.kind {
            sum(base, index);
        }
    }
    for statement in body.iter_mut() {
        match statement {
            Stmt::Store { place: Place::Memory { base, index, .. }, .. } => sum(base, index),
            Stmt::If { then_body, else_body, .. } => {
                unindexed_absolute(then_body, absolute);
                unindexed_absolute(else_body, absolute);
            }
            Stmt::Loop { body, step, effects, .. } => {
                unindexed_absolute(body, absolute);
                unindexed_absolute(step, absolute);
                unindexed_absolute(effects, absolute);
            }
            Stmt::Switch { arms, .. } => arms.iter_mut().for_each(|arm| unindexed_absolute(arm, absolute)),
            _ => {}
        }
    }
    for_each_expression(body, &mut |e| expression(e, &sum));
}

/// A pointer variable assigned only a frame object's address (`p = &v`,
/// an inlined call's `&local` argument): loads and stores through it
/// address the object directly. The variable keeps its other reads.
pub fn fold_frame_bases(function: &mut Function) {
    fn assignments(body: &[Stmt], out: &mut Vec<(VarId, Option<VarId>)>) {
        for statement in body {
            match statement {
                Stmt::Assign { variable, value } => out.push((
                    *variable,
                    match value.kind {
                        ExprKind::LocalAddress(object) => Some(object),
                        _ => None,
                    },
                )),
                Stmt::If { then_body, else_body, .. } => {
                    assignments(then_body, out);
                    assignments(else_body, out);
                }
                Stmt::Loop { body, step, effects, .. } => {
                    assignments(body, out);
                    assignments(step, out);
                    assignments(effects, out);
                }
                Stmt::Counted { body, .. } => assignments(body, out),
                Stmt::Switch { arms, .. } => arms.iter().for_each(|arm| assignments(arm, out)),
                _ => {}
            }
        }
    }
    let mut assigned = Vec::new();
    assignments(&function.body, &mut assigned);
    let folds: Vec<(VarId, VarId)> = assigned
        .iter()
        .filter_map(|&(variable, object)| Some((variable, object?)))
        .filter(|&(variable, _)| assigned.iter().filter(|&&(other, _)| other == variable).count() == 1)
        .filter(|&(variable, _)| {
            let local = &function.variables[variable];
            local.kind != VariableKind::Parameter && local.frame.is_none() && !local.volatile
        })
        .collect();
    if folds.is_empty() {
        return;
    }
    fn rebase(base: &mut Expr, folds: &[(VarId, VarId)]) {
        match &mut base.kind {
            ExprKind::Var(id) => {
                if let Some(&(_, object)) = folds.iter().find(|(variable, _)| variable == id) {
                    base.kind = ExprKind::LocalAddress(object);
                }
            }
            ExprKind::Binary(BinaryOp::Add | BinaryOp::Subtract, left, _) => rebase(left, folds),
            _ => {}
        }
    }
    fn expression(e: &mut Expr, folds: &[(VarId, VarId)]) {
        if let ExprKind::Load { base, .. } = &mut e.kind {
            rebase(base, folds);
        }
        children(e, &mut |child| expression(child, folds));
    }
    fn statements(body: &mut [Stmt], folds: &[(VarId, VarId)]) {
        for statement in body.iter_mut() {
            if let Stmt::Store { place: mwcc_iro::Place::Memory { base, .. }, .. } = statement {
                rebase(base, folds);
            }
            match statement {
                Stmt::If { then_body, else_body, .. } => {
                    statements(then_body, folds);
                    statements(else_body, folds);
                }
                Stmt::Loop { body, step, effects, .. } => {
                    statements(body, folds);
                    statements(step, folds);
                    statements(effects, folds);
                }
                Stmt::Counted { body, .. } => statements(body, folds),
                Stmt::Switch { arms, .. } => arms.iter_mut().for_each(|arm| statements(arm, folds)),
                _ => {}
            }
        }
    }
    statements(&mut function.body, &folds);
    for_each_expression(&mut function.body, &mut |e| expression(e, &folds));
}

/// Stores into a frame object nothing reads (no load of it, its address
/// never used as a value) are dead: they go, and with them its slot. An
/// address held only by a variable nothing reads is no use.
pub fn remove_unread_frame_stores(function: &mut Function) {
    // `reads[id]`: the variable's value, or the object's address, is used.
    fn expression(e: &Expr, reads: &mut [bool]) {
        if let ExprKind::LocalAddress(id) | ExprKind::Var(id) = e.kind {
            reads[id] = true;
        }
        let mut copy = e.clone();
        children(&mut copy, &mut |child| expression(child, reads));
    }
    // (`held`: an assigned address counts only when its variable is read.)
    fn statements(body: &[Stmt], reads: &mut [bool], held: Option<&[bool]>) {
        for statement in body {
            match statement {
                Stmt::Store { place: mwcc_iro::Place::Memory { base, index, .. }, value, .. } => {
                    // (A store's own base is no read.)
                    if !matches!(base.kind, ExprKind::LocalAddress(_)) {
                        expression(base, reads);
                    }
                    if let Some(index) = index {
                        expression(index, reads);
                    }
                    expression(value, reads);
                }
                Stmt::Store { value, .. } => expression(value, reads),
                Stmt::Assign { variable, value } => match (value.kind.clone(), held) {
                    (ExprKind::LocalAddress(object), Some(held)) => reads[object] |= held[*variable],
                    _ => expression(value, reads),
                },
                Stmt::Eval(value) | Stmt::SetReturn(value) => expression(value, reads),
                Stmt::Return(value) => {
                    if let Some(value) = value {
                        expression(value, reads);
                    }
                }
                Stmt::If { condition, then_body, else_body } => {
                    expression(condition, reads);
                    statements(then_body, reads, held);
                    statements(else_body, reads, held);
                }
                Stmt::Loop { condition, body, step, effects, .. } => {
                    if let Some(condition) = condition {
                        expression(condition, reads);
                    }
                    statements(body, reads, held);
                    statements(step, reads, held);
                    statements(effects, reads, held);
                }
                Stmt::Counted { count, guard, body } => {
                    expression(count, reads);
                    if let Some(guard) = guard {
                        expression(guard, reads);
                    }
                    statements(body, reads, held);
                }
                Stmt::Switch { value, arms, .. } => {
                    expression(value, reads);
                    arms.iter().for_each(|arm| statements(arm, reads, held));
                }
                _ => {}
            }
        }
    }
    let count = function.variables.len();
    let mut variable_reads = vec![false; count];
    statements(&function.body, &mut variable_reads, None);
    let mut object_reads = vec![false; count];
    statements(&function.body, &mut object_reads, Some(&variable_reads));
    let dead: Vec<bool> = function
        .variables
        .iter()
        .enumerate()
        .map(|(id, variable)| {
            variable.frame.is_some() && !variable.volatile && variable.kind != VariableKind::Parameter && !object_reads[id]
        })
        .collect();
    if !dead.iter().any(|&dead| dead) {
        return;
    }
    fn remove(body: &mut Vec<Stmt>, dead: &[bool], variable_reads: &[bool]) {
        body.retain(|statement| match statement {
            Stmt::Store { place: mwcc_iro::Place::Memory { base, index: None, .. }, value, .. } => {
                !(matches!(base.kind, ExprKind::LocalAddress(id) if dead[id]) && !format!("{value:?}").contains("Call"))
            }
            Stmt::Assign { variable, value } => {
                !(matches!(value.kind, ExprKind::LocalAddress(id) if dead[id]) && !variable_reads[*variable])
            }
            _ => true,
        });
        for statement in body.iter_mut() {
            match statement {
                Stmt::If { then_body, else_body, .. } => {
                    remove(then_body, dead, variable_reads);
                    remove(else_body, dead, variable_reads);
                }
                Stmt::Loop { body, step, effects, .. } => {
                    remove(body, dead, variable_reads);
                    remove(step, dead, variable_reads);
                    remove(effects, dead, variable_reads);
                }
                Stmt::Counted { body, .. } => remove(body, dead, variable_reads),
                Stmt::Switch { arms, .. } => arms.iter_mut().for_each(|arm| remove(arm, dead, variable_reads)),
                _ => {}
            }
        }
    }
    remove(&mut function.body, &dead, &variable_reads);
}

/// Replace every read of `variable` with `value` (a constant argument of
/// an inlined call).
pub fn substitute(body: &mut [Stmt], variable: VarId, value: &Expr) {
    fn expression(e: &mut Expr, variable: VarId, value: &Expr) {
        match &e.kind {
            ExprKind::Var(id) if *id == variable => *e = value.clone(),
            _ => children(e, &mut |child| expression(child, variable, value)),
        }
    }
    for_each_expression(body, &mut |e| expression(e, variable, value));
}

/// Renumber every variable reference (an inlined body moving into its
/// caller's numbering).
pub fn map_variables(body: &mut [Stmt], map: &dyn Fn(VarId) -> VarId) {
    fn expression(e: &mut Expr, map: &dyn Fn(VarId) -> VarId) {
        match &mut e.kind {
            ExprKind::Var(id) | ExprKind::LocalAddress(id) => *id = map(*id),
            _ => children(e, &mut |child| expression(child, map)),
        }
    }
    fn assigned(body: &mut [Stmt], map: &dyn Fn(VarId) -> VarId) {
        for statement in body {
            match statement {
                Stmt::Assign { variable, .. } => *variable = map(*variable),
                Stmt::If { then_body, else_body, .. } => {
                    assigned(then_body, map);
                    assigned(else_body, map);
                }
                Stmt::Loop { body, step, effects, .. } => {
                    assigned(body, map);
                    assigned(step, map);
                    assigned(effects, map);
                }
                Stmt::Counted { body, .. } => assigned(body, map),
                Stmt::Switch { arms, .. } => arms.iter_mut().for_each(|arm| assigned(arm, map)),
                _ => {}
            }
        }
    }
    for_each_expression(body, &mut |e| expression(e, map));
    assigned(body, map);
}

/// Renumber string literal references (an inline expansion's literals join
/// the caller's).
pub fn map_strings(body: &mut [Stmt], map: &dyn Fn(usize) -> usize) {
    fn expression(e: &mut Expr, map: &dyn Fn(usize) -> usize) {
        match &mut e.kind {
            ExprKind::StringAddress(index) => *index = map(*index),
            _ => children(e, &mut |child| expression(child, map)),
        }
    }
    for_each_expression(body, &mut |e| expression(e, map));
}

pub fn map_images(body: &mut [Stmt], map: &dyn Fn(usize) -> usize) {
    fn expression(e: &mut Expr, map: &dyn Fn(usize) -> usize) {
        match &mut e.kind {
            ExprKind::Image(index) => *index = map(*index),
            _ => children(e, &mut |child| expression(child, map)),
        }
    }
    for_each_expression(body, &mut |e| expression(e, map));
}

pub(crate) fn children(expression: &mut Expr, rewrite: &mut dyn FnMut(&mut Expr)) {
    match &mut expression.kind {
        ExprKind::Int(_)
        | ExprKind::Float(_)
        | ExprKind::Var(_)
        | ExprKind::Global(_)
        | ExprKind::GlobalAddress(_)
        | ExprKind::LocalAddress(_)
        | ExprKind::StringAddress(_) | ExprKind::Image(_) => {}
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
        ExprKind::Idiom(Idiom::Absolute(value) | Idiom::Unary(_, value)) => rewrite(value),
        ExprKind::Idiom(Idiom::Insert { base, value, .. } | Idiom::Update(_, base, value)) => {
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
    // A call through a known function's address calls it directly (an
    // inline expansion's function-pointer parameter).
    if let ExprKind::Call { name, arguments } = &expression.kind {
        if name == mwcc_iro::INDIRECT_CALL && std::env::var_os("MWCC_IRO_NO_DIRECT_KNOWN_CALLS").is_none() {
            if let Some(ExprKind::GlobalAddress(callee)) = arguments.first().map(|target| &target.kind) {
                return Some(Expr {
                    kind: ExprKind::Call { name: callee.clone(), arguments: arguments[1..].to_vec() },
                    ty: expression.ty,
                });
            }
        }
    }
    // A word operation's literal beyond 32 bits (a 64-bit parse-time fold
    // of `~(0xff << 24)`) contributes only its low word.
    if let ExprKind::Binary(op @ (BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor), left, right) = &expression.kind {
        let word = |e: &Expr| e.as_int().is_some_and(|value| i32::try_from(value).is_err() && u32::try_from(value).is_err());
        if mwcc_iro::is_general_word(expression.ty)
            && !mwcc_iro::is_wide(expression.ty)
            && (word(left) || word(right))
            && std::env::var_os("MWCC_IRO_NO_WORD_LITERALS").is_none()
        {
            let wrap = |e: &Expr| match e.as_int() {
                Some(value) if word(e) => Expr { kind: ExprKind::Int(i64::from(value as i32)), ty: e.ty },
                _ => e.clone(),
            };
            return Some(Expr { kind: ExprKind::Binary(*op, Box::new(wrap(left)), Box::new(wrap(right))), ty: expression.ty });
        }
    }
    match &expression.kind {
        // A literal widened to `long long` (by its own signedness), or a
        // wide literal narrowed to a word.
        ExprKind::Convert(operand) if mwcc_iro::is_wide(expression.ty) && operand.as_int().is_some() => {
            let value = operand.as_int()?;
            // (A signed literal is its value: folded words stay in range, and
            // a literal beyond them is exact; an unsigned word zero-extends.)
            let value = if !mwcc_iro::is_wide(operand.ty) && mwcc_iro::is_unsigned(operand.ty) {
                value & 0xffff_ffff
            } else {
                value
            };
            Some(Expr::typed_int(value, expression.ty))
        }
        ExprKind::Convert(operand)
            if matches!(expression.ty, Type::Int | Type::UnsignedInt)
                && mwcc_iro::is_wide(operand.ty)
                && operand.as_int().is_some() =>
        {
            let value = operand.as_int()?;
            Some(Expr::typed_int(if expression.ty == Type::UnsignedInt { value & 0xffff_ffff } else { i64::from(value as i32) }, expression.ty))
        }
        // A narrow cast of a literal truncates it.
        ExprKind::Convert(operand)
            if matches!(expression.ty, Type::Char | Type::UnsignedChar | Type::Short | Type::UnsignedShort)
                && operand.as_int().is_some()
                && std::env::var_os("MWCC_IRO_NO_NARROW_LITERALS").is_none() =>
        {
            let value = operand.as_int()?;
            let value = match expression.ty {
                Type::Char => i64::from(value as i8),
                Type::UnsignedChar => i64::from(value as u8),
                Type::Short => i64::from(value as i16),
                _ => i64::from(value as u16),
            };
            Some(Expr::typed_int(value, expression.ty))
        }
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
        // `~k` and `-k` of a literal word are literals.
        ExprKind::Unary(op @ (UnaryOp::BitNot | UnaryOp::Negate), operand)
            if operand.as_int().is_some() && mwcc_iro::is_general_word(expression.ty) && !mwcc_iro::is_wide(expression.ty) =>
        {
            let value = operand.as_int()? as i32;
            let folded = if *op == UnaryOp::BitNot { !value } else { value.wrapping_neg() };
            let value = if mwcc_iro::is_unsigned(expression.ty) { i64::from(folded as u32) } else { i64::from(folded) };
            Some(Expr::typed_int(value, expression.ty))
        }
        // `!!!x` is `!x`.
        ExprKind::Unary(UnaryOp::LogicalNot, operand)
            if matches!(&operand.kind, ExprKind::Unary(UnaryOp::LogicalNot, inner) if matches!(inner.kind, ExprKind::Unary(UnaryOp::LogicalNot, _))) =>
        {
            let ExprKind::Unary(_, inner) = &operand.kind else { return None };
            Some((**inner).clone())
        }
        // `-(x < 0)` is the sign mask `x >> 31`.
        ExprKind::Unary(UnaryOp::Negate, operand)
            if expression.ty == Type::Int
                && matches!(&operand.kind, ExprKind::Binary(BinaryOp::Less, x, zero)
                    if x.ty == Type::Int && zero.as_int() == Some(0))
                && std::env::var_os("MWCC_IRO_NO_SIGN_MASK_FOLD").is_none() =>
        {
            let ExprKind::Binary(_, x, _) = &operand.kind else { return None };
            Some(Expr::binary(BinaryOp::ShiftRight, (**x).clone(), Expr::typed_int(31, Type::Int), Type::Int))
        }
        // `-(-x)` and `~~x` are `x` (integers: GC/3.x negates a float twice).
        ExprKind::Unary(op @ (UnaryOp::Negate | UnaryOp::BitNot), operand)
            if matches!(&operand.kind, ExprKind::Unary(inner, _) if inner == op)
                && operand.ty == expression.ty
                && mwcc_iro::is_general_word(expression.ty) =>
        {
            let ExprKind::Unary(_, inner) = &operand.kind else { return None };
            let mut value = (**inner).clone();
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
            if left.as_int().is_some() && right.as_int().is_some() {
                if mwcc_iro::is_wide(expression.ty) || mwcc_iro::is_wide(left.ty) || mwcc_iro::is_wide(right.ty) {
                    return Some(Expr::typed_int(fold_wide(*op, left, right, expression.ty)?, expression.ty));
                }
                return Some(Expr::typed_int(fold_typed(*op, left, right)?, expression.ty));
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
            // `x * 1` is `x`; `x - x` is 0.
            if *op == BinaryOp::Multiply && right.as_int() == Some(1) && mwcc_iro::is_general_word(expression.ty) {
                let mut value = (**left).clone();
                value.ty = expression.ty;
                return Some(value);
            }
            if *op == BinaryOp::Subtract && matches!(left.kind, ExprKind::Var(_)) && same_leaf(left, right) && mwcc_iro::is_general_word(expression.ty) {
                return Some(Expr::typed_int(0, expression.ty));
            }
            // `-a * -b` is `a * b` (integers: GC/3.x negates floats).
            if matches!(op, BinaryOp::Multiply | BinaryOp::Divide) && mwcc_iro::is_general_word(expression.ty) {
                if let (ExprKind::Unary(UnaryOp::Negate, a), ExprKind::Unary(UnaryOp::Negate, b)) = (&left.kind, &right.kind) {
                    return Some(Expr { kind: ExprKind::Binary(*op, a.clone(), b.clone()), ty: expression.ty });
                }
            }
            if let Some(rewritten) = word_identities(*op, left, right, expression.ty) {
                return Some(rewritten);
            }
            // `x & ~0` is `x`.
            if *op == BinaryOp::BitAnd
                && matches!(expression.ty, Type::Int | Type::UnsignedInt)
                && right.as_int().is_some_and(|value| value as u32 == u32::MAX)
            {
                let mut value = (**left).clone();
                value.ty = expression.ty;
                return Some(value);
            }
            // Absorption: `(a & b) | a`, `a | (a & b)`, `(a | b) & a`, ... are `a`.
            if let Some(survivor) = absorbed(*op, left, right).or_else(|| absorbed(*op, right, left)) {
                let mut value = survivor.clone();
                value.ty = expression.ty;
                return Some(value);
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
/// Fold two literal operands; a relation between unsigned words compares
/// unsigned.
/// A `long long` operation on literals (64-bit, wrapping).
pub fn fold_wide(op: BinaryOp, left: &Expr, right: &Expr, ty: Type) -> Option<i64> {
    let (a, b) = (left.as_int()?, right.as_int()?);
    let unsigned = ty == Type::UnsignedLongLong || left.ty == Type::UnsignedLongLong || right.ty == Type::UnsignedLongLong;
    let value = match op {
        BinaryOp::Add => a.wrapping_add(b),
        BinaryOp::Subtract => a.wrapping_sub(b),
        BinaryOp::Multiply => a.wrapping_mul(b),
        BinaryOp::BitAnd => a & b,
        BinaryOp::BitOr => a | b,
        BinaryOp::BitXor => a ^ b,
        BinaryOp::ShiftLeft if (0..64).contains(&b) => a.wrapping_shl(b as u32),
        BinaryOp::ShiftRight if (0..64).contains(&b) && unsigned => ((a as u64) >> b) as i64,
        BinaryOp::ShiftRight if (0..64).contains(&b) => a >> b,
        BinaryOp::Divide if b != 0 && unsigned => ((a as u64) / (b as u64)) as i64,
        BinaryOp::Divide if b != 0 => a.wrapping_div(b),
        BinaryOp::Modulo if b != 0 && unsigned => ((a as u64) % (b as u64)) as i64,
        BinaryOp::Modulo if b != 0 => a.wrapping_rem(b),
        BinaryOp::Less => i64::from(if unsigned { (a as u64) < (b as u64) } else { a < b }),
        BinaryOp::Greater => i64::from(if unsigned { (a as u64) > (b as u64) } else { a > b }),
        BinaryOp::LessEqual => i64::from(if unsigned { (a as u64) <= (b as u64) } else { a <= b }),
        BinaryOp::GreaterEqual => i64::from(if unsigned { (a as u64) >= (b as u64) } else { a >= b }),
        BinaryOp::Equal => i64::from(a == b),
        BinaryOp::NotEqual => i64::from(a != b),
        _ => return None,
    };
    Some(value)
}

fn fold_typed(op: BinaryOp, left: &Expr, right: &Expr) -> Option<i64> {
    let (a, b) = (left.as_int()?, right.as_int()?);
    let unsigned = |ty: Type| mwcc_iro::is_unsigned(ty) && !mwcc_iro::is_narrow(ty);
    if unsigned(left.ty) || unsigned(right.ty) {
        let (a, b) = (a as u32, b as u32);
        let value = match op {
            BinaryOp::Less => Some(a < b),
            BinaryOp::Greater => Some(a > b),
            BinaryOp::LessEqual => Some(a <= b),
            BinaryOp::GreaterEqual => Some(a >= b),
            _ => None,
        };
        if let Some(value) = value {
            return Some(i64::from(value));
        }
        let value = match op {
            BinaryOp::Divide if b != 0 => Some(a / b),
            BinaryOp::Modulo if b != 0 => Some(a % b),
            BinaryOp::ShiftRight if b < 32 && unsigned(left.ty) => Some(a >> b),
            _ => None,
        };
        if let Some(value) = value {
            return Some(i64::from(value as i32));
        }
    }
    let (x, y) = (a as i32, b as i32);
    let value = match op {
        BinaryOp::Divide if y != 0 => Some(x.wrapping_div(y)),
        BinaryOp::Modulo if y != 0 => Some(x.wrapping_rem(y)),
        BinaryOp::ShiftRight if (0..32).contains(&y) && !unsigned(left.ty) => Some(x >> y),
        _ => None,
    };
    if let Some(value) = value.filter(|_| std::env::var_os("MWCC_IRO_NO_DIVISION_FOLDS").is_none()) {
        return Some(i64::from(value));
    }
    fold_literals(op, a, b)
}

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
    // (Not across a widening to `long long`: the inner word wraps first.)
    if mwcc_iro::is_wide(ty) != mwcc_iro::is_wide(left.ty) {
        return None;
    }
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
    // (`(b + a) - b` is `a`.)
    if *inner == BinaryOp::Add && op == BinaryOp::Subtract && same_leaf(x, right) {
        return Some(y.as_ref());
    }
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
        | ExprKind::StringAddress(_) | ExprKind::Image(_) => true,
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

    // An unsigned word above zero is a nonzero one (`u > 0` = `u != 0`).
    if let ExprKind::Binary(op, left, right) = &mut expression.kind {
        let (relation, value) = match (left.as_int(), right.as_int()) {
            (None, Some(0)) => (*op, &**left),
            (Some(0), None) => (op.mirror(), &**right),
            _ => (*op, &**left),
        };
        // (Or a zero-extended unsigned narrow value.)
        let word = (mwcc_iro::is_unsigned(value.ty) && !mwcc_iro::is_narrow(value.ty))
            || (matches!(&value.kind, ExprKind::Convert(inner) if matches!(inner.ty, Type::UnsignedChar | Type::UnsignedShort))
                && std::env::var_os("MWCC_IRO_NO_NARROW_NONZERO").is_none());
        if word && (left.as_int() == Some(0)) != (right.as_int() == Some(0)) {
            match relation {
                BinaryOp::Greater => *op = BinaryOp::NotEqual,
                BinaryOp::LessEqual => *op = BinaryOp::Equal,
                _ => {}
            }
        }
    }
    // A mask keeping every bit of a zero-extended unsigned narrow value
    // (`u8 & 0xFF`, `u16 & 0xFFFF`) is the value.
    if let ExprKind::Binary(BinaryOp::BitAnd, value, mask) = &expression.kind {
        let Some(mask) = mask.as_int() else { return };
        let width = match &value.kind {
            ExprKind::Convert(inner) if inner.ty == Type::UnsignedChar => 0xff,
            ExprKind::Convert(inner) if inner.ty == Type::UnsignedShort => 0xffff,
            _ => return,
        };
        if mask & width == width && std::env::var_os("MWCC_IRO_NO_NARROW_MASK_FOLD").is_none() {
            let ty = expression.ty;
            let value = value.as_ref().clone();
            *expression = if value.ty == ty { value } else { Expr { kind: ExprKind::Convert(Box::new(value)), ty } };
        }
    }
}

/// MWCC's reassociation of integer sums: a variable added to a sum goes
/// after it (`a + (b + c)` = `(b + c) + a`); `(x + k) + y` = `(x + y) + k`;
/// `(x + y) + z` = `x + (y + z)` when `x` is a variable, else `y + (x + z)`
/// when `y` is one.
fn reassociate_sum(expression: &mut Expr) {
    let word = |ty: Type| matches!(ty, Type::Int | Type::UnsignedInt);
    if !word(expression.ty) {
        return;
    }
    let ty = expression.ty;
    let ExprKind::Binary(BinaryOp::Add, left, right) = &mut expression.kind else { return };
    let sum = |e: &Expr| matches!(e.kind, ExprKind::Binary(BinaryOp::Add, ..)) && word(e.ty);
    if left.as_var().is_some() && sum(right) {
        std::mem::swap(left, right);
    }
    let (left, right) = (left.as_ref().clone(), right.as_ref().clone());
    let ExprKind::Binary(BinaryOp::Add, x, y) = &left.kind else { return };
    if !word(left.ty) {
        return;
    }
    let (x, y) = (x.as_ref().clone(), y.as_ref().clone());
    if y.as_int().is_some() {
        if right.as_int().is_none() {
            *expression = Expr::binary(BinaryOp::Add, Expr::binary(BinaryOp::Add, x, right, ty), y, ty);
        }
        return;
    }
    if x.as_var().is_some() {
        *expression = Expr::binary(BinaryOp::Add, x, Expr::binary(BinaryOp::Add, y, right, ty), ty);
    } else if y.as_var().is_some() {
        *expression = Expr::binary(BinaryOp::Add, y, Expr::binary(BinaryOp::Add, x, right, ty), ty);
    }
}

/// `a - -b` = `a + b`, `a + -b` = `a - b`, `-a + b` = `b - a`,
/// `-a - b` = `-(a + b)`.
fn negation(expression: &Expr) -> Option<Expr> {
    let ExprKind::Binary(op, left, right) = &expression.kind else { return None };
    // (Not for `long long`: a negated word widens after negating.)
    if mwcc_iro::is_wide(expression.ty) {
        return None;
    }
    let negated = |e: &Expr| match &e.kind {
        ExprKind::Unary(UnaryOp::Negate, operand) => Some((**operand).clone()),
        _ => None,
    };
    let ty = expression.ty;
    // (GC/3.x keeps every floating negation; earlier builds fold them, but
    // a negated variable on the left of a sum stays.)
    if mwcc_iro::is_float(ty) && !FLOAT_NEGATION_ALGEBRA.with(|flag| flag.get()) {
        return None;
    }
    match op {
        BinaryOp::Subtract => {
            if let Some(b) = negated(right) {
                return Some(Expr::binary(BinaryOp::Add, (**left).clone(), b, ty));
            }
            // (Not for floating values: `-0 - -0` is +0, `-(0 + -0)` is -0.)
            if mwcc_iro::is_float(ty) {
                return None;
            }
            let a = negated(left)?;
            Some(Expr::unary(UnaryOp::Negate, Expr::binary(BinaryOp::Add, a, (**right).clone(), ty), ty))
        }
        BinaryOp::Add => {
            if let Some(b) = negated(right) {
                return Some(Expr::binary(BinaryOp::Subtract, (**left).clone(), b, ty));
            }
            // (A floating `-a + b` keeps a negated variable.)
            if mwcc_iro::is_float(ty) && negated(left).is_some_and(|a| matches!(a.kind, ExprKind::Var(_))) {
                return None;
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
                Idiom::Absolute(_) | Idiom::Insert { .. } | Idiom::Unary(..) | Idiom::Update(..) => Type::Int,
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
        Idiom::Insert { .. } | Idiom::Unary(..) | Idiom::Update(..) => return None,
    };
    let equality = matches!(idiom, Idiom::Masked { relation: BinaryOp::Equal | BinaryOp::NotEqual, .. });
    let ty = match tested.as_var() {
        Some(id) => variables[id],
        None => tested.ty,
    };
    (ty == Type::Int || equality && matches!(ty, Type::UnsignedInt | Type::Pointer(_))).then_some(idiom)
}

fn recognize(condition: &Expr, when_true: &Expr, when_false: &Expr) -> Option<Idiom> {
    // A variable, or arithmetic on variables and constants (`(s16)(x << 1)`).
    fn pure(e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Var(_) | ExprKind::Int(_) => true,
            ExprKind::Convert(operand) => !mwcc_iro::is_float(operand.ty) && pure(operand),
            ExprKind::Unary(op, operand) => *op != UnaryOp::LogicalNot && pure(operand),
            ExprKind::Binary(op, left, right) => {
                !op.is_comparison()
                    && !matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr | BinaryOp::Divide | BinaryOp::Modulo)
                    && pure(left)
                    && pure(right)
            }
            _ => false,
        }
    }
    let expressions = std::env::var_os("MWCC_IRO_VARIABLE_IDIOMS_ONLY").is_none();
    let is_var = |e: &Expr| e.as_var().is_some() || (expressions && e.as_int().is_none() && !mwcc_iro::is_float(e.ty) && pure(e));
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
    let key = format!("{:?}", tested.kind);
    let equality = matches!(relation, BinaryOp::Equal | BinaryOp::NotEqual);
    let negation_of = |e: &Expr| matches!(&e.kind, ExprKind::Unary(UnaryOp::Negate, operand) if format!("{:?}", operand.kind) == key);
    let same = |e: &Expr| format!("{:?}", e.kind) == key;
    let negative_side = matches!(relation, BinaryOp::Less | BinaryOp::LessEqual);
    // (Only a variable's absolute value is branch-free.)
    if !equality
        && tested.as_var().is_some()
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
            Stmt::Loop { body, step, effects, .. } => {
                rewrite_selects(function, body, false);
                rewrite_selects(function, step, false);
                rewrite_selects(function, effects, false);
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
                    // (Constants stored to one global: a branch-free value
                    // stored once.)
                    (
                        [Stmt::Store { place: Place::Global(name), ty, value, .. }],
                        [Stmt::Store { place: Place::Global(other), ty: other_ty, value: other_value, .. }],
                    ) if name == other
                        && ty == other_ty
                        && value.as_int().zip(other_value.as_int()).is_some_and(|(a, b)| (a - b).abs() == 1 || (a == 0 && b == -1) || (a == -1 && b == 0))
                        && std::env::var_os("MWCC_IRO_NO_GLOBAL_STORE_SELECTS").is_none() =>
                    {
                        let (name, ty) = (name.clone(), *ty);
                        let temporary = function.add_temporary(Type::Int);
                        if let Some(rewritten) = select(function, &condition, value, other_value, Destination::Variable(temporary)) {
                            output.extend(rewritten);
                            output.push(Stmt::Store {
                                place: Place::Global(name),
                                ty,
                                value: Expr { kind: ExprKind::Var(temporary), ty: Type::Int },
                                compound: false,
                            });
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
    // Constants one apart: `c ? k + 1 : k` is `k + truth(c)`, `c ? k : k + 1`
    // is `(k + 1) - truth(c)` (a mask).
    if let (Some(a), Some(b)) = (when_true.as_int(), when_false.as_int()) {
        let integer_condition = match &condition.kind {
            ExprKind::Binary(op, left, _) if op.is_comparison() => !mwcc_iro::is_float(left.ty) && !mwcc_iro::is_wide(left.ty),
            _ => mwcc_iro::is_general_word(condition.ty) && !mwcc_iro::is_wide(condition.ty),
        };
        // (Not 0/1, a truth value, nor 0/-1, a sign mask; nor a logical
        // condition, which has no value form.)
        let logical = format!("{condition:?}").contains("Logical");
        let handled = matches!((a, b), (0, 1) | (1, 0) | (0, -1) | (-1, 0));
        if (a - b).abs() == 1
            && integer_condition
            && !logical
            && !handled
            && std::env::var_os("MWCC_IRO_NO_CONSECUTIVE_SELECTS").is_none()
        {
            let truth = match &condition.kind {
                ExprKind::Binary(op, ..) if op.is_comparison() => Expr { ty: Type::Int, ..condition.clone() },
                _ => Expr::binary(BinaryOp::NotEqual, condition.clone(), Expr::int(0), Type::Int),
            };
            // (`x >= 0 ? k : k + 1` takes the sign: `k + (x < 0)`.)
            let (truth, a, b) = match &truth.kind {
                ExprKind::Binary(BinaryOp::GreaterEqual, x, zero) if zero.as_int() == Some(0) && a == b - 1 => {
                    (Expr::binary(BinaryOp::Less, (**x).clone(), Expr::int(0), Type::Int), b, a)
                }
                _ => (truth, a, b),
            };
            // (A mask against zero is an idiom, kept from the algebra.)
            let term = if a == b + 1 {
                truth
            } else {
                match &truth.kind {
                    ExprKind::Binary(relation, x, zero) if zero.as_int() == Some(0) && x.ty == Type::Int => Expr {
                        kind: ExprKind::Idiom(Idiom::Masked {
                            relation: *relation,
                            tested: x.clone(),
                            value: Box::new(Expr::int(-1)),
                            keep_when_true: true,
                        }),
                        ty: Type::Int,
                    },
                    _ => Expr::unary(UnaryOp::Negate, truth, Type::Int),
                }
            };
            let value = Expr::binary(BinaryOp::Add, term, Expr::int(b), Type::Int);
            return Some(vec![destination.assign(value)]);
        }
    }
    // `x < 0 ? -1 : 0` (and `x >= 0 ? 0 : -1`) is the sign mask `x >> 31`.
    if let ExprKind::Binary(op @ (BinaryOp::Less | BinaryOp::GreaterEqual), x, zero) = &condition.kind {
        let arms = (when_true.as_int(), when_false.as_int());
        let matches = match op {
            BinaryOp::Less => arms == (Some(-1), Some(0)),
            _ => arms == (Some(0), Some(-1)),
        };
        if matches && zero.as_int() == Some(0) && x.ty == Type::Int && std::env::var_os("MWCC_IRO_NO_SIGN_MASK_SELECT").is_none() {
            let mask = Expr::binary(BinaryOp::ShiftRight, (**x).clone(), Expr::typed_int(31, Type::Int), Type::Int);
            return Some(vec![destination.assign(mask)]);
        }
    }
    let variables: Vec<Type> = function.variables.iter().map(|variable| variable.ty).collect();
    if let Some(idiom) = sign_idiom(condition, when_true, when_false, &variables) {
        let ty = match &idiom {
            Idiom::Absolute(_) | Idiom::Insert { .. } | Idiom::Unary(..) | Idiom::Update(..) => Type::Int,
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
/// (`optimized`: the narrowed-shift and dead-mask rules apply.)
pub fn stores(body: &mut [Stmt], optimized: bool) {
    for statement in body {
        match statement {
            Stmt::Store { ty, value, .. } => {
                let stored = width(*ty);
                while let ExprKind::Convert(operand) = &value.kind {
                    // (A narrowed right shift stays: it is one rotate-and-mask,
                    // `(u8)(x >> 8)` -> `rlwinm 24,24,31`.)
                    let narrowed_shift = optimized
                        && is_narrow(value.ty)
                        && matches!(&operand.kind, ExprKind::Binary(BinaryOp::ShiftRight, shifted, count)
                            if count.as_int().is_some() && mwcc_iro::is_unsigned(shifted.ty))
                        && std::env::var_os("MWCC_IRO_DEAD_SHIFT_NARROWING").is_none();
                    if narrowed_shift
                        || !(mwcc_iro::is_general_word(value.ty) && !matches!(value.ty, Type::Pointer(_) | Type::StructPointer { .. }))
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
                // (A mask keeping every stored bit is dead: `*p = b & 0xff`.)
                if optimized && is_narrow(*ty) && std::env::var_os("MWCC_IRO_LIVE_STORE_MASKS").is_none() {
                    let low = (1i64 << (8 * stored)) - 1;
                    if let ExprKind::Binary(BinaryOp::BitAnd, left, right) = &value.kind {
                        if right.as_int().is_some_and(|mask| mask & low == low) && !mwcc_iro::is_float(left.ty) {
                            *value = (**left).clone();
                        }
                    }
                }
            }
            Stmt::If { then_body, else_body, .. } => {
                stores(then_body, optimized);
                stores(else_body, optimized);
            }
            Stmt::Loop { body, step, effects, .. } => {
                stores(body, optimized);
                stores(step, optimized);
                stores(effects, optimized);
            }
            Stmt::Switch { arms, .. } => {
                for arm in arms {
                    stores(arm, optimized);
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
    let maximum: i64 = match ty {
        Type::Char => 127,
        Type::UnsignedChar => 255,
        Type::Short => 32767,
        _ => 65535,
    };
    match &value.kind {
        // A mask within the type's range leaves nothing to convert.
        ExprKind::Binary(BinaryOp::BitAnd, _, mask)
            if mask.as_int().is_some_and(|mask| (0..=maximum).contains(&mask))
                && std::env::var_os("MWCC_IRO_NO_MASK_FITS").is_none() =>
        {
            true
        }
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
                Stmt::Loop { body, step, effects, .. } => {
                    // A definition inside a loop runs repeatedly.
                    let mut inner = vec![0; counts.len()];
                    count_assignments(body, &mut inner);
                    count_assignments(step, &mut inner);
                    count_assignments(effects, &mut inner);
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
                Stmt::Loop { body, step, effects, .. } => {
                    remove_definitions(body, forwarded);
                    remove_definitions(step, forwarded);
                    remove_definitions(effects, forwarded);
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
    // `x = E + k` with E not an unassigned variable: x keeps E (assigned in
    // place) and its uses read `x + k`.
    let mut rebased: HashMap<VarId, Expr> = HashMap::new();
    for (variable, value) in defs {
        if counts[variable] != 1 || function.variables[variable].kind != VariableKind::Local {
            continue;
        }
        // (Also `(u32)p - k` assigned to a pointer: the same offset address.)
        let (value, base, k) = match &value.kind {
            ExprKind::Binary(BinaryOp::Add, base, offset) if offset.as_int().is_some() => (value.clone(), base.clone(), offset.as_int().unwrap_or(0)),
            ExprKind::Binary(BinaryOp::Subtract, base, offset)
                if offset.as_int().is_some()
                    && (matches!(value.ty, Type::Int | Type::UnsignedInt) || pointer_like(value.ty))
                    && pointer_like(function.variables[variable].ty)
                    && matches!(&base.kind, ExprKind::Convert(inner) if matches!(inner.kind, ExprKind::Var(_)) && pointer_like(inner.ty))
                    && std::env::var_os("MWCC_IRO_NO_WORD_OFFSETS").is_none() =>
            {
                let ExprKind::Convert(inner) = &base.kind else { continue };
                let ty = function.variables[variable].ty;
                let k = -offset.as_int().unwrap_or(0);
                let base = Box::new(Expr { ty, ..(**inner).clone() });
                let rebuilt = Expr::binary(BinaryOp::Add, (*base).clone(), Expr::typed_int(k, Type::Int), ty);
                (rebuilt, base, k)
            }
            _ => continue,
        };
        let offset = Expr::typed_int(k, Type::Int);
        if !pointer_like(value.ty) || !pointer_like(base.ty) {
            continue;
        }
        match &base.kind {
            ExprKind::Var(source) if counts[*source] == 0 => {
                forwarded.insert(variable, value.clone());
            }
            ExprKind::Var(_) => {}
            _ if std::env::var_os("MWCC_IRO_NO_REBASED_OFFSETS").is_none() => {
                let ty = value.ty;
                rebased.insert(
                    variable,
                    Expr::binary(BinaryOp::Add, Expr { kind: ExprKind::Var(variable), ty }, Expr::typed_int(k, offset.ty), ty),
                );
            }
            _ => {}
        }
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
                Stmt::Loop { condition, body, step, effects, .. } => {
                    if let Some(condition) = condition {
                        value_uses(condition, false, out);
                    }
                    statement_value_uses(body, out);
                    statement_value_uses(step, out);
                    statement_value_uses(effects, out);
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
    rebased.retain(|variable, _| !escaping.contains(variable));
    if !rebased.is_empty() {
        // The definition keeps the base; every (address) use adds k.
        fn rebase_uses(expression: &mut Expr, rebased: &HashMap<VarId, Expr>) {
            if let ExprKind::Var(id) = expression.kind {
                if let Some(value) = rebased.get(&id) {
                    *expression = value.clone();
                    return;
                }
            }
            children(expression, &mut |child| rebase_uses(child, rebased));
        }
        fn rebase(body: &mut [Stmt], rebased: &HashMap<VarId, Expr>) {
            for statement in body.iter_mut() {
                match statement {
                    Stmt::Assign { variable, value } if rebased.contains_key(variable) => {
                        let ExprKind::Binary(BinaryOp::Add, base, _) = &value.kind else { continue };
                        *value = base.as_ref().clone();
                        rebase_uses(value, rebased);
                    }
                    Stmt::If { condition, then_body, else_body } => {
                        rebase_uses(condition, rebased);
                        rebase(then_body, rebased);
                        rebase(else_body, rebased);
                    }
                    Stmt::Loop { condition, body, step, effects, .. } => {
                        if let Some(condition) = condition {
                            rebase_uses(condition, rebased);
                        }
                        rebase(body, rebased);
                        rebase(step, rebased);
                        rebase(effects, rebased);
                    }
                    Stmt::Switch { value, arms, .. } => {
                        rebase_uses(value, rebased);
                        for arm in arms.iter_mut() {
                            rebase(arm, rebased);
                        }
                    }
                    other => for_each_expression(std::slice::from_mut(other), &mut |e| rebase_uses(e, rebased)),
                }
            }
        }
        rebase(&mut function.body, &rebased);
    }
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

/// A word variable updated in place by a constant (`x = x + k`, `x <<= k`,
/// `x &= k`, ...) in the function's top-level statements keeps its old
/// value: every later use reads `x op k` instead, up to the variable's next
/// top-level assignment (whose value reads it the same way, so successive
/// updates fold). Not when a nested statement assigns it before then.
pub fn forward_updates(function: &mut Function) {
    fn assigns(body: &[Stmt], variable: VarId) -> bool {
        body.iter().any(|statement| match statement {
            Stmt::Assign { variable: assigned, .. } => *assigned == variable,
            Stmt::If { then_body, else_body, .. } => assigns(then_body, variable) || assigns(else_body, variable),
            Stmt::Loop { body, step, effects, .. } => {
                assigns(body, variable) || assigns(step, variable) || assigns(effects, variable)
            }
            Stmt::Counted { body, .. } => assigns(body, variable),
            Stmt::Switch { arms, .. } => arms.iter().any(|arm| assigns(arm, variable)),
            _ => false,
        })
    }
    fn assigned_in_loop(body: &[Stmt], variable: VarId) -> bool {
        body.iter().any(|statement| match statement {
            Stmt::If { then_body, else_body, .. } => {
                assigned_in_loop(then_body, variable) || assigned_in_loop(else_body, variable)
            }
            Stmt::Loop { body, step, effects, .. } => {
                assigns(body, variable) || assigns(step, variable) || assigns(effects, variable)
            }
            Stmt::Counted { body, .. } => assigns(body, variable),
            Stmt::Switch { arms, .. } => arms.iter().any(|arm| assigned_in_loop(arm, variable)),
            _ => false,
        })
    }
    fn addressed(expression: &Expr, variable: VarId) -> bool {
        if matches!(expression.kind, ExprKind::LocalAddress(id) if id == variable) {
            return true;
        }
        let mut found = false;
        let mut copy = expression.clone();
        children(&mut copy, &mut |child| found |= addressed(child, variable));
        found
    }
    let mut index = 0;
    while index < function.body.len() {
        let Stmt::Assign { variable, value } = &function.body[index] else {
            index += 1;
            continue;
        };
        let (variable, value) = (*variable, value.clone());
        let word = matches!(value.ty, Type::Int | Type::UnsignedInt) || pointer_like(value.ty);
        let update = matches!(&value.kind,
            ExprKind::Binary(
                BinaryOp::Add | BinaryOp::Subtract | BinaryOp::ShiftLeft | BinaryOp::ShiftRight | BinaryOp::BitAnd | BinaryOp::BitOr | BinaryOp::BitXor,
                left,
                right,
            ) if matches!(left.kind, ExprKind::Var(id) if id == variable) && right.as_int().is_some());
        let candidate = &function.variables[variable];
        let eligible = word
            && update
            && matches!(candidate.kind, VariableKind::Local | VariableKind::Parameter)
            && candidate.frame.is_none()
            && !candidate.volatile
            && !candidate.raw;
        if !eligible {
            index += 1;
            continue;
        }
        // The statements reading the update: to the next top-level
        // assignment, none nested before it.
        let rest = &function.body[index + 1..];
        let end = rest
            .iter()
            .position(|statement| matches!(statement, Stmt::Assign { variable: assigned, .. } if *assigned == variable))
            .unwrap_or(rest.len());
        let mut addressed_anywhere = false;
        for_each_expression(&mut function.body.clone(), &mut |e| addressed_anywhere |= addressed(e, variable));
        // (Nor a variable a loop assigns.)
        if addressed_anywhere || assigns(&rest[..end], variable) || assigned_in_loop(&function.body, variable) {
            index += 1;
            continue;
        }
        let stop = index + 1 + end;
        let limit = (stop + 1).min(function.body.len());
        for statement in &mut function.body[index + 1..limit] {
            substitute(std::slice::from_mut(statement), variable, &value);
        }
        // (The next definition reads the old value too.)
        if let Some(Stmt::Assign { value: next, .. }) = function.body.get_mut(stop) {
            fold(next);
        }
        function.body.remove(index);
    }
}

/// The same variable or literal.
fn same_leaf(first: &Expr, second: &Expr) -> bool {
    match (&first.kind, &second.kind) {
        (ExprKind::Var(a), ExprKind::Var(b)) => a == b,
        (ExprKind::Int(a), ExprKind::Int(b)) => a == b,
        _ => false,
    }
}

/// `outer op (… inner …)` that absorbs to `outer`: `a | (a & b)`,
/// `a & (a | b)` (either inner order), with `a` a variable and `b` free
/// of effects.
fn absorbed<'a>(op: BinaryOp, outer: &'a Expr, inner: &Expr) -> Option<&'a Expr> {
    let inner_op = match op {
        BinaryOp::BitOr => BinaryOp::BitAnd,
        BinaryOp::BitAnd => BinaryOp::BitOr,
        _ => return None,
    };
    let ExprKind::Binary(found, x, y) = &inner.kind else { return None };
    if *found != inner_op || !matches!(outer.kind, ExprKind::Var(_)) {
        return None;
    }
    ((same_leaf(x, outer) && speculable(y)) || (same_leaf(y, outer) && speculable(x))).then_some(outer)
}

/// Before GC/3.x, floating negations cancel: `-(-x)` is `x`, `-a * -b` is
/// `a * b` (and so for division).
pub fn float_negations(body: &mut [Stmt]) {
    fn rewrite(expression: &mut Expr) {
        children(expression, &mut |child| rewrite(child));
        if !matches!(expression.ty, Type::Float | Type::Double) {
            return;
        }
        let replacement = match &expression.kind {
            ExprKind::Unary(UnaryOp::Negate, operand) => match &operand.kind {
                ExprKind::Unary(UnaryOp::Negate, inner) if inner.ty == expression.ty => Some((**inner).clone()),
                _ => None,
            },
            ExprKind::Binary(op @ (BinaryOp::Multiply | BinaryOp::Divide), left, right) => match (&left.kind, &right.kind) {
                (ExprKind::Unary(UnaryOp::Negate, a), ExprKind::Unary(UnaryOp::Negate, b)) => {
                    Some(Expr { kind: ExprKind::Binary(*op, a.clone(), b.clone()), ty: expression.ty })
                }
                _ => None,
            },
            _ => None,
        };
        if let Some(replacement) = replacement {
            *expression = replacement;
        }
    }
    for_each_expression(body, &mut |expression| rewrite(expression));
}

/// Word identities MWCC's IRO applies: De Morgan (`~a & ~b` is `~(a | b)`),
/// a shift pair as a mask (`x << k >> k`), distributed constant products
/// (`a*3 + a*5`), merged masks (`(x & m) | (x & n)`), `-a - k` as `-k - a`,
/// and `2^k == (x & 2^k)` with the constant right.
fn word_identities(op: BinaryOp, left: &Expr, right: &Expr, ty: Type) -> Option<Expr> {
    if !matches!(ty, Type::Int | Type::UnsignedInt) || std::env::var_os("MWCC_IRO_NO_WORD_IDENTITIES").is_some() {
        return None;
    }
    let var = |e: &Expr| matches!(e.kind, ExprKind::Var(_));
    match (op, &left.kind, &right.kind) {
        (BinaryOp::BitAnd | BinaryOp::BitOr, ExprKind::Unary(UnaryOp::BitNot, a), ExprKind::Unary(UnaryOp::BitNot, b)) => {
            let inner = if op == BinaryOp::BitAnd { BinaryOp::BitOr } else { BinaryOp::BitAnd };
            Some(Expr::unary(UnaryOp::BitNot, Expr::binary(inner, (**a).clone(), (**b).clone(), ty), ty))
        }
        (BinaryOp::ShiftRight, ExprKind::Binary(BinaryOp::ShiftLeft, x, k), _)
            if ty == Type::UnsignedInt && x.ty == Type::UnsignedInt && k.as_int().is_some_and(|k| (1..32).contains(&k)) && k.as_int() == right.as_int() =>
        {
            let mask = u32::MAX >> k.as_int()?;
            Some(Expr::binary(BinaryOp::BitAnd, (**x).clone(), Expr::typed_int(i64::from(mask), ty), ty))
        }
        // (Different counts: a shift and a mask.)
        (BinaryOp::ShiftRight, ExprKind::Binary(BinaryOp::ShiftLeft, x, k), _)
            if ty == Type::UnsignedInt && x.ty == Type::UnsignedInt && k.as_int().is_some_and(|k| (1..32).contains(&k))
                && right.as_int().is_some_and(|n| (1..32).contains(&n)) =>
        {
            let (k, n) = (k.as_int()?, right.as_int()?);
            let mask = i64::from((u32::MAX << k) >> n);
            let shifted = if k > n {
                Expr::binary(BinaryOp::ShiftLeft, (**x).clone(), Expr::typed_int(k - n, Type::Int), ty)
            } else {
                Expr::binary(BinaryOp::ShiftRight, (**x).clone(), Expr::typed_int(n - k, Type::Int), ty)
            };
            Some(Expr::binary(BinaryOp::BitAnd, shifted, Expr::typed_int(mask, ty), ty))
        }
        (BinaryOp::Add | BinaryOp::Subtract, ExprKind::Binary(BinaryOp::Multiply, a, k1), ExprKind::Binary(BinaryOp::Multiply, b, k2))
            if var(a) && same_leaf(a, b) =>
        {
            let (k1, k2) = (k1.as_int()?, k2.as_int()?);
            let k = if op == BinaryOp::Add { k1 + k2 } else { k1 - k2 };
            Some(Expr::binary(BinaryOp::Multiply, (**a).clone(), Expr::typed_int(i64::from(k as i32), ty), ty))
        }
        (BinaryOp::BitOr, ExprKind::Binary(BinaryOp::BitAnd, a, m), ExprKind::Binary(BinaryOp::BitAnd, b, n)) if var(a) && same_leaf(a, b) => {
            let (m, n) = (m.as_int()?, n.as_int()?);
            Some(Expr::binary(BinaryOp::BitAnd, (**a).clone(), Expr::typed_int(m | n, ty), ty))
        }
        (BinaryOp::Subtract, ExprKind::Unary(UnaryOp::Negate, a), _) if right.as_int().is_some() => {
            let k = right.as_int()?;
            Some(Expr::binary(BinaryOp::Subtract, Expr::typed_int(i64::from((k as i32).wrapping_neg()), ty), (**a).clone(), ty))
        }
        // `(x | c) & m` with c covering m is m; `(x & c) | m` with m
        // covering c is m.
        (BinaryOp::BitAnd, ExprKind::Binary(BinaryOp::BitOr, x, c), _)
            if right.as_int().is_some_and(|m| c.as_int().is_some_and(|c| c & m == m)) && speculable(x) =>
        {
            Some(Expr::typed_int(right.as_int()?, ty))
        }
        (BinaryOp::BitOr, ExprKind::Binary(BinaryOp::BitAnd, x, c), _)
            if right.as_int().is_some_and(|m| c.as_int().is_some_and(|c| c & m == c)) && speculable(x) =>
        {
            Some(Expr::typed_int(right.as_int()?, ty))
        }
        // `(int)(u >> n) & m`: the retyping changes no bits (the shift's
        // kind comes from its operand).
        (BinaryOp::BitAnd, ExprKind::Convert(inner), ExprKind::Int(_))
            if matches!(inner.ty, Type::Int | Type::UnsignedInt) && matches!(inner.kind, ExprKind::Binary(BinaryOp::ShiftRight, ..)) =>
        {
            let inner = Expr { ty, ..(**inner).clone() };
            Some(Expr::binary(BinaryOp::BitAnd, inner, right.clone(), ty))
        }
        // `(x & m) >> n` with m keeping every bit the shift keeps (logical).
        (BinaryOp::ShiftRight, ExprKind::Binary(BinaryOp::BitAnd, x, m), ExprKind::Int(n))
            if ty == Type::UnsignedInt && (1..32).contains(n) && m.as_int().is_some_and(|m| (m as u32) | (u32::MAX >> (32 - n)) == u32::MAX) =>
        {
            Some(Expr::binary(BinaryOp::ShiftRight, (**x).clone(), right.clone(), ty))
        }
        // `x - (k << 16)` is `addis x,-k`.
        (BinaryOp::Subtract, _, ExprKind::Int(k)) if *k as u32 & 0xffff == 0 && *k as u32 != 0 && *k as i32 != i32::MIN => {
            Some(Expr::binary(BinaryOp::Add, left.clone(), Expr::typed_int(i64::from((*k as i32).wrapping_neg()), ty), ty))
        }
        (BinaryOp::Equal | BinaryOp::NotEqual, ExprKind::Int(_), _) if right.as_int().is_none() => {
            Some(Expr::binary(op, right.clone(), left.clone(), ty))
        }
        _ => None,
    }
}

/// After the early builds, `(x & 2^k) == 2^k` is bit k of `x`
/// (`rlwinm x,32-k,31,31`).
pub fn bit_tests(body: &mut [Stmt]) {
    fn rewrite(expression: &mut Expr) {
        children(expression, &mut |child| rewrite(child));
        let ExprKind::Binary(BinaryOp::Equal, left, right) = &expression.kind else { return };
        let ExprKind::Binary(BinaryOp::BitAnd, x, mask) = &left.kind else { return };
        let (Some(m), Some(v)) = (mask.as_int(), right.as_int()) else { return };
        let m = m as u32;
        // (`(x & 2^k) == 0` is bit k flipped.)
        if v == 0 && m.is_power_of_two() && m > 1 && mwcc_iro::is_general_word(x.ty) && !is_narrow(x.ty) && std::env::var_os("MWCC_IRO_NO_CLEAR_BIT_TESTS").is_none() {
            let k = Expr::typed_int(i64::from(m.trailing_zeros()), Type::Int);
            let shifted = Expr::binary(BinaryOp::ShiftRight, Expr { ty: Type::UnsignedInt, ..(**x).clone() }, k, Type::UnsignedInt);
            let bit = Expr::binary(BinaryOp::BitAnd, shifted, Expr::typed_int(1, Type::UnsignedInt), Type::UnsignedInt);
            let mut flipped = Expr::binary(BinaryOp::BitXor, bit, Expr::typed_int(1, Type::UnsignedInt), Type::UnsignedInt);
            flipped.ty = Type::Int;
            *expression = flipped;
            return;
        }
        let fits = !is_narrow(x.ty) || m < 1u32 << (8 * width(x.ty));
        if m != v as u32 || !m.is_power_of_two() || m < 2 || !mwcc_iro::is_general_word(x.ty) || !fits {
            return;
        }
        let k = Expr::typed_int(i64::from(m.trailing_zeros()), Type::Int);
        let shifted = Expr::binary(BinaryOp::ShiftRight, (**x).clone(), k, x.ty);
        let mut bit = Expr::binary(BinaryOp::BitAnd, shifted, Expr::typed_int(1, x.ty), x.ty);
        bit.ty = Type::Int;
        *expression = bit;
    }
    // (A tested condition keeps its comparison: `rlwinm.` and a branch.)
    fn walk(body: &mut [Stmt]) {
        for statement in body.iter_mut() {
            match statement {
                Stmt::If { condition, then_body, else_body } => {
                    children(condition, &mut |child| rewrite(child));
                    walk(then_body);
                    walk(else_body);
                }
                Stmt::Loop { condition, body, step, effects, .. } => {
                    if let Some(condition) = condition {
                        children(condition, &mut |child| rewrite(child));
                    }
                    walk(body);
                    walk(step);
                    walk(effects);
                }
                Stmt::Counted { count, guard, body } => {
                    rewrite(count);
                    if let Some(guard) = guard {
                        children(guard, &mut |child| rewrite(child));
                    }
                    walk(body);
                }
                Stmt::Switch { value, arms, .. } => {
                    rewrite(value);
                    for arm in arms {
                        walk(arm);
                    }
                }
                other => for_each_expression(std::slice::from_mut(other), &mut |expression| rewrite(expression)),
            }
        }
    }
    walk(body);
}

/// Sums of variables carry their constants last: `(a - 1) + b` is
/// `(a + b) - 1`, `(a + 1) + (b - 3)` is `(b + a) - 2`; and a common factor
/// comes out: `a*b + a*c` is `a * (b + c)`. GC/1.x-2.x (`variable_first`)
/// put the constant term's variable first in `b + (a - 1)`; GC/3.x keeps
/// source order.
pub fn hoist_constants(expression: &mut Expr, variable_first: bool) {
    children(expression, &mut |child| hoist_constants(child, variable_first));
    let ty = expression.ty;
    if !matches!(ty, Type::Int | Type::UnsignedInt) {
        return;
    }
    let var = |e: &Expr| matches!(e.kind, ExprKind::Var(_)) && matches!(e.ty, Type::Int | Type::UnsignedInt);
    // (GC/1.x-2.x: `(a + b) - k` takes k from b: `a + (b - k)`.)
    if variable_first {
        if let ExprKind::Binary(BinaryOp::Subtract, sum, k) = &expression.kind {
            if let (ExprKind::Binary(BinaryOp::Add, a, b), Some(k)) = (&sum.kind, k.as_int()) {
                if var(a) && var(b) {
                    let inner = Expr::binary(BinaryOp::Subtract, (**b).clone(), Expr::typed_int(k, ty), ty);
                    *expression = Expr::binary(BinaryOp::Add, (**a).clone(), inner, ty);
                    return;
                }
            }
        }
    }
    let ExprKind::Binary(BinaryOp::Add, left, right) = &expression.kind else { return };
    // `x + c` / `x - c` with x a variable: (x, signed c).
    let offset = |e: &Expr| -> Option<(Expr, i64)> {
        let ExprKind::Binary(op @ (BinaryOp::Add | BinaryOp::Subtract), x, c) = &e.kind else { return None };
        // (GC/1.x-2.x reassociate `x + k` themselves.)
        if variable_first && *op == BinaryOp::Add {
            return None;
        }
        let c = c.as_int()?;
        var(x).then(|| ((**x).clone(), if *op == BinaryOp::Add { c } else { -c }))
    };
    let rebuilt = match (offset(left), offset(right)) {
        (Some((x, c)), None) if var(right) => Some((x, (**right).clone(), c)),
        (None, Some((x, c))) if var(left) => Some(if variable_first { (x, (**left).clone(), c) } else { ((**left).clone(), x, c) }),
        (Some((x, c1)), Some((y, c2))) => Some((y, x, c1 + c2)),
        _ => None,
    };
    if let Some((first, second, c)) = rebuilt {
        let sum = Expr::binary(BinaryOp::Add, first, second, ty);
        let c = i64::from(c as i32);
        *expression = Expr::binary(BinaryOp::Add, sum, Expr::typed_int(c, ty), ty);
        return;
    }
    // `a*b + a*c` (any operand order) is `a * (b + c)`.
    let (ExprKind::Binary(BinaryOp::Multiply, p, q), ExprKind::Binary(BinaryOp::Multiply, r, t)) = (&left.kind, &right.kind) else { return };
    if ![p, q, r, t].iter().all(|e| var(e)) {
        return;
    }
    let factored = if same_leaf(p, r) {
        Some((p, q, t))
    } else if same_leaf(p, t) {
        Some((p, q, r))
    } else if same_leaf(q, r) {
        Some((q, p, t))
    } else if same_leaf(q, t) {
        Some((q, p, r))
    } else {
        None
    };
    if let Some((common, b, c)) = factored {
        let sum = Expr::binary(BinaryOp::Add, (**b).clone(), (**c).clone(), ty);
        // (GC/1.x-2.x keep the factor where the first product has it.)
        let factor_right = variable_first && same_leaf(common, q) && !same_leaf(common, p);
        *expression = if factor_right {
            Expr::binary(BinaryOp::Multiply, sum, (**common).clone(), ty)
        } else {
            Expr::binary(BinaryOp::Multiply, (**common).clone(), sum, ty)
        };
    }
}

/// A local assigned once from a call-free value and read once, in the very
/// next top-level statement, is replaced by its value there (MWCC propagates
/// the expression into its single use).
pub fn forward_single_uses(function: &mut Function) {
    fn reads(body: &[Stmt], variable: VarId) -> usize {
        let mut count = 0;
        let mut copy = body.to_vec();
        for_each_expression(&mut copy, &mut |e| {
            fn walk(e: &Expr, variable: VarId, count: &mut usize) {
                if matches!(e.kind, ExprKind::Var(id) if id == variable) {
                    *count += 1;
                }
                let mut copy = e.clone();
                children(&mut copy, &mut |child| walk(child, variable, count));
            }
            walk(e, variable, &mut count);
        });
        count
    }
    let mut index = 0;
    while index + 1 < function.body.len() {
        let Stmt::Assign { variable, value } = &function.body[index] else {
            index += 1;
            continue;
        };
        let (variable, value) = (*variable, value.clone());
        let candidate = &function.variables[variable];
        // (A pointer read-modify-written through: `s = p->q; s->f = v`,
        // whose reads all go through it.)
        let through = read_modify_write(&function.body[index + 1], variable)
            && matches!(value.kind, ExprKind::Load { .. })
            && !toggle_env("MWCC_IRO_NO_FORWARDED_RMW_BASE");
        let uses = if through { 2 } else { 1 };
        let eligible = matches!(candidate.kind, VariableKind::Local)
            && candidate.frame.is_none()
            && !candidate.volatile
            && !candidate.raw
            && !format!("{value:?}").contains("Call {")
            // (Nor a read of memory past another: their order holds.)
            && (through || !(reads_memory(&value) && reads_memory_in(&function.body[index + 1])))
            && !value.mentions(variable)
            && reads(&function.body, variable) == uses
            && reads(std::slice::from_ref(&function.body[index + 1]), variable) == uses
            && matches!(&function.body[index + 1], Stmt::Store { .. } | Stmt::Assign { .. } | Stmt::Eval(_))
            && function.body.iter().filter(|s| matches!(s, Stmt::Assign { variable: v, .. } if *v == variable)).count() == 1;
        if !eligible {
            index += 1;
            continue;
        }
        substitute(std::slice::from_mut(&mut function.body[index + 1]), variable, &value);
        function.body.remove(index);
    }
}

/// GC/1.3-2.x subtract a sign-extended word with a zero high word, unless
/// an earlier statement already extended the same word (CSE reuses that):
/// `*o = (s64)n; return v - (s64)n;` subtracts the true extension.
pub fn shared_wide_subtrahends(body: &mut [Stmt]) {
    fn extended(e: &Expr, seen: &mut Vec<String>) {
        if let ExprKind::Convert(word) = &e.kind {
            if e.ty == Type::LongLong && !mwcc_iro::is_wide(word.ty) && !matches!(word.kind, ExprKind::Convert(_)) {
                seen.push(format!("{word:?}"));
            }
        }
        let mut copy = e.clone();
        children(&mut copy, &mut |child| extended(child, seen));
    }
    fn restore(e: &mut Expr, seen: &[String]) {
        if let ExprKind::Convert(unsigned) = &e.kind {
            if e.ty == Type::LongLong && unsigned.ty == Type::UnsignedInt {
                if let ExprKind::Convert(word) = &unsigned.kind {
                    if !mwcc_iro::is_unsigned(word.ty) && seen.contains(&format!("{word:?}")) {
                        let word = (**word).clone();
                        *e = Expr { kind: ExprKind::Convert(Box::new(word)), ty: Type::LongLong };
                        return;
                    }
                }
            }
        }
        children(e, &mut |child| restore(child, seen));
    }
    let mut seen: Vec<String> = Vec::new();
    for statement in body.iter_mut() {
        if !seen.is_empty() {
            for_each_expression(std::slice::from_mut(statement), &mut |e| restore(e, &seen));
        }
        match statement {
            Stmt::Assign { .. } | Stmt::Store { .. } | Stmt::Eval(_) | Stmt::SetReturn(_) | Stmt::Return(_) => {
                let mut copy = vec![statement.clone()];
                for_each_expression(&mut copy, &mut |e| extended(e, &mut seen));
                // (A word stored to a wide place extends there.)
                if let Stmt::Store { ty: Type::LongLong, value, .. } = statement {
                    if !mwcc_iro::is_wide(value.ty) && !mwcc_iro::is_unsigned(value.ty) {
                        seen.push(format!("{value:?}"));
                    }
                }
                // (An assignment changes what its variable extends to.)
                if let Stmt::Assign { variable, .. } = statement {
                    let name = format!("{:?}", ExprKind::Var(*variable));
                    seen.retain(|key| !key.contains(&name));
                }
            }
            // (Only straight-line code.)
            _ => seen.clear(),
        }
    }
}

fn toggle_env(name: &str) -> bool {
    std::env::var_os(name).is_some()
}

/// `*(v + k) = f(*(v + k))`: a store through `variable` whose value reads
/// memory only there, without calls.
fn read_modify_write(statement: &Stmt, variable: VarId) -> bool {
    let Stmt::Store { place: Place::Memory { base, index: None, offset }, value, .. } = statement else { return false };
    if !matches!(base.kind, ExprKind::Var(id) if id == variable) || format!("{value:?}").contains("Call {") {
        return false;
    }
    fn only_there(e: &Expr, variable: VarId, offset: i32) -> bool {
        match &e.kind {
            ExprKind::Load { base, index: None, offset: at } => {
                *at == offset && matches!(base.kind, ExprKind::Var(id) if id == variable)
            }
            ExprKind::Load { .. } | ExprKind::Global(_) => false,
            _ => {
                let mut ok = true;
                let mut copy = e.clone();
                children(&mut copy, &mut |child| ok &= only_there(child, variable, offset));
                ok
            }
        }
    }
    only_there(value, variable, *offset) && reads_memory(value)
}

fn reads_memory(expression: &Expr) -> bool {
    let listing = format!("{expression:?}");
    listing.contains("Load {") || listing.contains("Global(")
}

fn reads_memory_in(statement: &Stmt) -> bool {
    // (A call may change memory: a read moved into its statement could
    // follow it.)
    let listing = format!("{statement:?}");
    listing.contains("Load {") || listing.contains("Global(") || listing.contains("Call {")
}

thread_local! {
    /// Floating negations fold into sums (GC/1.x-2.x); set per build.
    pub static FLOAT_NEGATION_ALGEBRA: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// A member array element (`p->words[i]`): MWCC adds the scaled index to the
/// struct pointer and keeps the member offset as the displacement
/// (`slwi; add; lwz 4(r)`), not `addi` then an indexed access.
pub fn member_index_displacements(body: &mut [Stmt]) {
    fn rebase(base: &mut Box<Expr>, index: &mut Option<Box<Expr>>, offset: &mut i32) {
        if index.is_none() {
            return;
        }
        let ExprKind::Binary(BinaryOp::Add, pointer, member) = &base.kind else { return };
        let Some(k) = member.as_int() else { return };
        if !pointer_like(pointer.ty) || !(1..0x8000).contains(&(k + i64::from(*offset))) {
            return;
        }
        let ty = base.ty;
        let pointer = (**pointer).clone();
        let index_expr = *index.take().expect("checked");
        **base = Expr::binary(BinaryOp::Add, pointer, index_expr, ty);
        *offset += k as i32;
    }
    fn rewrite(expression: &mut Expr) {
        children(expression, &mut |child| rewrite(child));
        if let ExprKind::Load { base, index, offset } = &mut expression.kind {
            rebase(base, index, offset);
        }
    }
    for statement in body.iter_mut() {
        if let Stmt::Store { place: Place::Memory { base, index, offset }, .. } = statement {
            rebase(base, index, offset);
        }
        match statement {
            Stmt::If { then_body, else_body, .. } => {
                member_index_displacements(then_body);
                member_index_displacements(else_body);
            }
            Stmt::Loop { body, step, effects, .. } => {
                member_index_displacements(body);
                member_index_displacements(step);
                member_index_displacements(effects);
            }
            Stmt::Counted { body, .. } => member_index_displacements(body),
            Stmt::Switch { arms, .. } => {
                for arm in arms {
                    member_index_displacements(arm);
                }
            }
            _ => {}
        }
    }
    for_each_expression(body, &mut |expression| rewrite(expression));
}
