//! AST normalization passes and shared helpers (free functions and plan types).

#[allow(unused_imports)]
use super::*;


/// How a run of constant stores materializes its values (see `constant_store_run_plan`). `AllSame`
/// reuses the scratch register for one repeated `li`; `Distinct` gives each store's value its own
/// register (materialized up front, r(N+1) descending to r3 with the last in r0), stored in source
/// order.
pub(crate) enum ConstStoreRun {
    AllSame,
    Distinct(Vec<(i32, u8)>),
}

/// The `(operand, constant)` a guard condition compares against, when it is `<var> OP <const>`
/// (or the commuted `<const> OP <var>`). Two consecutive guards with the same key share one
/// `cmpwi` in mwcc, which emit_guard_sequence does not model (so it defers such a pair).
pub(crate) fn guard_comparison_key(condition: &Expression) -> Option<(String, i64)> {
    let Expression::Binary { operator, left, right } = condition else { return None };
    if !matches!(
        operator,
        BinaryOperator::Less
            | BinaryOperator::Greater
            | BinaryOperator::LessEqual
            | BinaryOperator::GreaterEqual
            | BinaryOperator::Equal
            | BinaryOperator::NotEqual
    ) {
        return None;
    }
    if let (Expression::Variable(name), Some(constant)) = (left.as_ref(), constant_value(right)) {
        return Some((name.clone(), constant));
    }
    if let (Some(constant), Expression::Variable(name)) = (constant_value(left), right.as_ref()) {
        return Some((name.clone(), constant));
    }
    None
}

/// A safe-when-nonzero access of a register pointer `p` — `*p`, `p[const]`, or `p->field` — the kind
/// of dereference a null guard protects. (A variable index `p[i]` needs its scaled register live, so
/// it is excluded.)
pub(crate) fn accesses_pointer(expression: &Expression, pointer: &str) -> bool {
    let is_pointer = |expression: &Expression| matches!(expression, Expression::Variable(name) if name == pointer);
    match expression {
        Expression::Dereference { pointer: inner } => is_pointer(inner.as_ref()),
        Expression::Index { base, index } => is_pointer(base.as_ref()) && constant_value(index).is_some(),
        Expression::Member { base, .. } => {
            is_pointer(base.as_ref())
                || matches!(base.as_ref(), Expression::Dereference { pointer: inner } if is_pointer(inner.as_ref()))
        }
        _ => false,
    }
}

/// A null-guarded dereference: a guard `!p` / `p` for a register pointer p, with one arm a CONSTANT
/// and the other a safe-when-nonzero access of that p (`*p`, `p[const]`, `p->field`). Returns
/// `(pointer, hot_access, cold_constant)`. mwcc branches on `p == 0` to the cold constant and puts the
/// access in the fall-through — it cannot fold to a branchless select because dereferencing null is
/// unsafe. Int-width return only (a narrow return sign-extends even the cold constant, a byte diff).
pub(crate) fn guarded_null_dereference<'a>(condition: &'a Expression, value: &'a Expression, default: &'a Expression, return_type: Type) -> Option<(&'a str, &'a Expression, &'a Expression)> {
    // int/unsigned or a narrow int (char/short): the cold constant is truncated and loaded directly
    // (no over-extension) and each hot access loads at its natural width (lbz/lha/lwz).
    if !matches!(return_type, Type::Int | Type::UnsignedInt) && !is_narrow_int(return_type) {
        return None;
    }
    match condition {
        // `if (!p) return VALUE; return DEFAULT;` — p == 0 yields the constant VALUE (cold), p != 0
        // yields the DEFAULT access of p (hot).
        Expression::Unary { operator: UnaryOperator::LogicalNot, operand } => {
            if let Expression::Variable(pointer) = operand.as_ref() {
                if constant_value(value).is_some() && accesses_pointer(default, pointer) {
                    return Some((pointer.as_str(), default, value));
                }
            }
        }
        // `if (p) return VALUE; return DEFAULT;` — p != 0 yields the VALUE access of p (hot), p == 0
        // yields the constant DEFAULT (cold).
        Expression::Variable(pointer) => {
            if accesses_pointer(value, pointer) && constant_value(default).is_some() {
                return Some((pointer.as_str(), value, default));
            }
        }
        _ => {}
    }
    None
}

/// The branchless select for a guard `if (cond) return value;` with fall-through
/// `default`. mwcc keeps the *guard value* as the in-place default, so a negated
/// guard `if (!c) ...` is compiled by stripping the `!` and swapping the arms —
/// `(c) ? default : value` — not as the bare `(!c) ? value : default` a ternary
/// would (mwcc normalizes only on the guard path, not a written ternary).
pub(crate) fn guard_select(condition: &Expression, value: &Expression, default: &Expression) -> Expression {
    if let Expression::Unary { operator: UnaryOperator::LogicalNot, operand } = condition {
        Expression::Conditional {
            condition: Box::new((**operand).clone()),
            when_true: Box::new(default.clone()),
            when_false: Box::new(value.clone()),
        }
    } else {
        Expression::Conditional {
            condition: Box::new(condition.clone()),
            when_true: Box::new(value.clone()),
            when_false: Box::new(default.clone()),
        }
    }
}

/// Whether a statement references (reads, writes, or takes the address of) `name`.
/// Control-flow statements are treated conservatively as referencing everything.
pub(crate) fn statement_references_name(statement: &Statement, name: &str) -> bool {
    match statement {
        // Jumps redirect control anywhere — conservative, like the other
        // control-flow arms below.
        Statement::Break | Statement::Continue | Statement::Goto(_) | Statement::Label(_) => true,
        Statement::Store { target, value } => expression_reads_name(target, name) || expression_reads_name(value, name),
        Statement::Assign { name: target, value } => target == name || expression_reads_name(value, name),
        Statement::Expression(expression) => expression_reads_name(expression, name),
        Statement::If { .. } | Statement::Switch { .. } | Statement::Loop { .. } | Statement::Return(_) => true,
    }
}

/// Drop locals that are never referenced anywhere and whose initializer has no side
/// effect (no call) — mwcc eliminates an unused `int s = 0;`, emitting no `li`. A
/// referenced local (read, written, or address-taken — any use of its name), or a
/// call-initialized one (whose call must still run), is kept.
pub(crate) fn remove_dead_locals(function: &Function) -> Option<Function> {
    if function.locals.is_empty() {
        return None;
    }
    let referenced = |name: &str| -> bool {
        function.locals.iter().any(|local| {
            local.name != name && local.initializer.as_ref().map_or(false, |init| expression_reads_name(init, name))
        }) || function.statements.iter().any(|statement| statement_references_name(statement, name))
            || function.guards.iter().any(|guard| {
                expression_reads_name(&guard.condition, name) || expression_reads_name(&guard.value, name)
            })
            || function.return_expression.as_ref().map_or(false, |ret| expression_reads_name(ret, name))
    };
    let kept: Vec<LocalDeclaration> = function
        .locals
        .iter()
        .filter(|local| referenced(&local.name) || local.initializer.as_ref().map_or(false, |init| expression_has_call(init)))
        .cloned()
        .collect();
    if kept.len() == function.locals.len() {
        return None;
    }
    Some(Function { locals: kept, ..function.clone() })
}

/// A DEAD trailing local whose initializer has a side effect (`int x = g();` where x is never read):
/// mwcc keeps the call but discards the result. Convert it to a leading expression statement so the
/// ordinary call/return paths emit it — `int x=g(); return a+b;` becomes `g(); return a+b;`. Keeping
/// it as a local would let the callee-saved paths (which emit only statements + the return, not local
/// initializers) silently DROP the call — a miscompile. Only the LAST local converts: its initializer
/// runs after every other local's initializer and before the statements, exactly where a leading
/// statement runs, so the order is preserved; a re-run converts several trailing dead-call locals in
/// order (each new statement prepends before the previous, reconstructing L0..Ln).
pub(crate) fn hoist_dead_trailing_call_local(function: &Function) -> Option<Function> {
    let last = function.locals.last()?;
    let name = last.name.clone();
    let initializer = last.initializer.clone()?;
    if !expression_has_call(&initializer) {
        return None;
    }
    // Dead: not read by any earlier local's initializer, a statement, a guard, or the return.
    let read_elsewhere = function
        .locals
        .iter()
        .rev()
        .skip(1)
        .any(|local| local.initializer.as_ref().map_or(false, |init| expression_reads_name(init, &name)))
        || function.statements.iter().any(|statement| statement_references_name(statement, &name))
        || function.guards.iter().any(|guard| {
            expression_reads_name(&guard.condition, &name) || expression_reads_name(&guard.value, &name)
        })
        || function.return_expression.as_ref().map_or(false, |ret| expression_reads_name(ret, &name));
    if read_elsewhere {
        return None;
    }
    let mut locals = function.locals.clone();
    locals.pop();
    let mut statements = function.statements.clone();
    statements.insert(0, Statement::Expression(initializer));
    Some(Function { locals, statements, ..function.clone() })
}

/// Fold a pure function-pointer alias local into the single call THROUGH it: `F t = gf;
/// t();` compiles exactly like `gf();` (mwcc loads the pointer right before `mtctr`
/// either way — the load position is unchanged). Only the exactly-safe shape folds: the
/// alias's ONLY use is as the call target of the FIRST statement (a later call-through
/// would observe a possibly-rewritten global; a read anywhere else needs the register
/// allocation the fold erases).
pub(crate) fn inline_first_call_target_alias(function: &Function) -> Option<Function> {
    if function.locals.len() != 1 {
        return None;
    }
    let local = &function.locals[0];
    if local.is_static {
        return None;
    }
    let Some(Expression::Variable(target)) = &local.initializer else {
        return None;
    };
    let Some(Statement::Expression(Expression::Call { name, arguments })) = function.statements.first() else {
        return None;
    };
    if name != &local.name {
        return None;
    }
    let reads_local = |expression: &Expression| expression_reads_name(expression, &local.name);
    if arguments.iter().any(reads_local)
        || function.statements[1..].iter().any(|statement| statement_references_name(statement, &local.name))
        || function.guards.iter().any(|guard| reads_local(&guard.condition) || reads_local(&guard.value))
        || function.return_expression.as_ref().is_some_and(reads_local)
    {
        return None;
    }
    let mut statements = function.statements.clone();
    statements[0] = Statement::Expression(Expression::Call { name: target.clone(), arguments: arguments.clone() });
    Some(Function { locals: Vec::new(), statements, ..function.clone() })
}

/// Fold single-assignment, return-only locals (whose initializers make no call) into
/// the return expression, dropping them — so `int z = x + 1; g(); return z;` becomes
/// the equivalent `g(); return x + 1;`, which the parameter-preservation path compiles.
/// Only a call-making body whose locals are pure return aliases qualifies; a local
/// initialized by a call (preserved as a call result), reassigned, read by a statement,
/// or feeding control flow leaves the function unchanged (`None`).
/// Inline register locals whose function routes through the FRAME-RESIDENT path
/// (an address-taken variable is present): that path evaluates the body directly
/// and cannot bind register locals, but with each read-once, call-free local
/// substituted away the body is the direct form it already compiles byte-exactly
/// (`int hx = *(int*)&x; return hx & C;` -> `return (*(int*)&x) & C;`). Leaf,
/// statement-free bodies only: a call could rewrite the punned memory, a store
/// could alias it, and a twice-read local would duplicate its load.
/// Reads of `name` in a statement's NON-CONDITION positions (store targets and
/// values, assign values, if-block bodies) — the if CONDITION is counted in the
/// dedup-safe bucket by the caller.
/// Substitute values into every expression position of a statement (recursing
/// into if-blocks).
pub(crate) fn substitute_statement(statement: &Statement, values: &std::collections::HashMap<String, Expression>) -> Statement {
    match statement {
        Statement::Store { target, value } => Statement::Store {
            target: crate::value_tracking::substitute(target, values),
            value: crate::value_tracking::substitute(value, values),
        },
        Statement::Assign { name, value } => Statement::Assign {
            name: name.clone(),
            value: crate::value_tracking::substitute(value, values),
        },
        Statement::If { condition, then_body, else_body } => Statement::If {
            condition: crate::value_tracking::substitute(condition, values),
            then_body: then_body.iter().map(|inner| substitute_statement(inner, values)).collect(),
            else_body: else_body.iter().map(|inner| substitute_statement(inner, values)).collect(),
        },
        other => other.clone(),
    }
}

pub(crate) fn statement_reads(statement: &Statement, name: &str) -> usize {
    match statement {
        Statement::Store { target, value } => count_name_occurrences(target, name) + count_name_occurrences(value, name),
        Statement::Assign { value, .. } => count_name_occurrences(value, name),
        Statement::If { then_body, else_body, .. } => {
            then_body.iter().map(|inner| statement_reads(inner, name)).sum::<usize>()
                + else_body.iter().map(|inner| statement_reads(inner, name)).sum::<usize>()
        }
        _ => 0,
    }
}

/// A dereference whose pointer reduces to a cast/offset around `&variable` — the
/// type-punned frame read (`*(int*)&x`, `*(1+(int*)&x)`). Pure and side-effect
/// free, so re-emitting it is only a duplicated load.
pub(crate) fn is_punned_frame_read(expression: &Expression) -> bool {
    fn is_address_of_variable(pointer: &Expression) -> bool {
        match pointer {
            Expression::AddressOf { operand } => matches!(operand.as_ref(), Expression::Variable(_)),
            Expression::Cast { operand, .. } => is_address_of_variable(operand),
            Expression::Binary { operator: BinaryOperator::Add | BinaryOperator::Subtract, left, right } => {
                (constant_value(left).is_some() && is_address_of_variable(right))
                    || (constant_value(right).is_some() && is_address_of_variable(left))
            }
            _ => false,
        }
    }
    match expression {
        Expression::Dereference { pointer } => is_address_of_variable(pointer),
        // The masked word (`hx & 0x7fffffff`) shares its punned load AND the mask
        // through the guard-chain emitter, so it is dedup-safe in guard conditions
        // the same way the bare read is.
        Expression::Binary { operator: BinaryOperator::BitAnd, left, right } => {
            constant_value(right).is_some() && is_punned_frame_read(left)
        }
        _ => false,
    }
}

/// See `lower_function`: reads of static const float/double globals become their
/// literal values (mwcc de-names them into the anonymous constant pool).
pub(crate) fn substitute_const_float_globals(function: &Function, globals: &[mwcc_syntax_trees::GlobalDeclaration]) -> Option<Function> {
    let shadowed: std::collections::HashSet<&str> = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.as_str())
        .chain(function.locals.iter().map(|local| local.name.as_str()))
        .collect();
    let values: std::collections::HashMap<String, Expression> = globals
        .iter()
        .filter(|global| global.is_const && global.is_static && global.array_length.is_none())
        .filter(|global| !shadowed.contains(global.name.as_str()))
        .filter_map(|global| {
            let bits = *global.initializer.as_ref()?.first()?;
            let value = match global.declared_type {
                Type::Double => f64::from_bits(bits as u64),
                Type::Float => f32::from_bits(bits as u32) as f64,
                _ => return None,
            };
            Some((global.name.clone(), Expression::FloatLiteral(value)))
        })
        .collect();
    if values.is_empty() {
        return None;
    }
    let reads_any = |expression: &Expression| values.keys().any(|name| expression_reads_name(expression, name));
    let mut touched = false;
    let map = |expression: &Expression, touched: &mut bool| {
        if reads_any(expression) {
            *touched = true;
            crate::value_tracking::substitute(expression, &values)
        } else {
            expression.clone()
        }
    };
    fn map_statement(statement: &Statement, map: &mut dyn FnMut(&Expression) -> Expression) -> Statement {
        match statement {
            Statement::Store { target, value } => Statement::Store { target: map(target), value: map(value) },
            Statement::Assign { name, value } => Statement::Assign { name: name.clone(), value: map(value) },
            Statement::Expression(expression) => Statement::Expression(map(expression)),
            Statement::If { condition, then_body, else_body } => Statement::If {
                condition: map(condition),
                then_body: then_body.iter().map(|inner| map_statement(inner, map)).collect(),
                else_body: else_body.iter().map(|inner| map_statement(inner, map)).collect(),
            },
            Statement::Return(value) => Statement::Return(value.as_ref().map(map)),
            other => other.clone(),
        }
    }
    let mut map_expression = |expression: &Expression| map(expression, &mut touched);
    let function = Function {
        return_type: function.return_type,
        section: function.section.clone(),
        asm_body: None, force_active: false,
        name: function.name.clone(),
        is_static: function.is_static,
        is_weak: function.is_weak,
        parameters: function.parameters.clone(),
        locals: function
            .locals
            .iter()
            .map(|local| LocalDeclaration { initializer: local.initializer.as_ref().map(&mut map_expression), ..local.clone() })
            .collect(),
        statements: function.statements.iter().map(|statement| map_statement(statement, &mut map_expression)).collect(),
        guards: function
            .guards
            .iter()
            .map(|guard| GuardedReturn { condition: map_expression(&guard.condition), value: map_expression(&guard.value) })
            .collect(),
        return_expression: function.return_expression.as_ref().map(&mut map_expression),
    };
    touched.then_some(function)
}

pub(crate) fn inline_frame_feeding_locals(function: &Function) -> Option<Function> {
    if function.locals.is_empty() {
        return None;
    }
    // Store statements may ride along (frexp's `*eptr = 0;` before its guards),
    // as may a single-level If whose body is stores/assigns (the writeback
    // block); their reads count toward each local's read budget below. Other
    // statement kinds keep the pass out.
    // A statement ASSIGNING a local would read back a stale substituted value —
    // those bodies (the frexp family) belong to the frame path, not this pass.
    let local_names: std::collections::HashSet<&str> = function.locals.iter().map(|local| local.name.as_str()).collect();
    let assigns_local = |statement: &Statement| match statement {
        Statement::Assign { name, .. } => local_names.contains(name.as_str()),
        _ => false,
    };
    let simple = |statement: &Statement| matches!(statement, Statement::Store { .. } | Statement::Assign { .. });
    if !function.statements.iter().all(|statement| match statement {
        Statement::Store { .. } => true,
        Statement::If { then_body, else_body, .. } => {
            then_body.iter().all(|inner| simple(inner) && !assigns_local(inner))
                && else_body.iter().all(|inner| simple(inner) && !assigns_local(inner))
        }
        _ => false,
    }) {
        return None;
    }
    if function_makes_call(function) {
        return None;
    }
    let address_taken = crate::frame::collect_address_taken(function);
    if address_taken.is_empty() {
        return None;
    }
    let return_expression = function.return_expression.as_ref()?;
    for local in &function.locals {
        // Only REGISTER locals inline; an address-taken or array local is the frame
        // path's own business (and a register local must be full width — a narrow
        // local carries a truncation substitution would drop).
        if address_taken.contains(local.name.as_str()) || local.array_length.is_some() {
            return None;
        }
        if local.declared_type.width() < 32 {
            return None;
        }
    }
    let mut values: std::collections::HashMap<String, Expression> = std::collections::HashMap::new();
    for (index, local) in function.locals.iter().enumerate() {
        let initializer = local.initializer.as_ref()?;
        if expression_has_call(initializer) {
            return None;
        }
        // Read-once: across the later initializers, the guards, and the return, so
        // the substitution cannot duplicate a load mwcc would keep in a register.
        // EXCEPTION: a pure punned frame read may repeat across GUARD CONDITIONS —
        // the frame guard-chain emitter shares one loaded word down the chain (and
        // any chain it cannot share defers at classification), so the duplication
        // never reaches the object.
        let guard_condition_reads = function
            .guards
            .iter()
            .map(|guard| count_name_occurrences(&guard.condition, &local.name))
            .sum::<usize>()
            + function
                .statements
                .iter()
                .map(|statement| match statement {
                    Statement::If { condition, .. } => count_name_occurrences(condition, &local.name),
                    _ => 0,
                })
                .sum::<usize>();
        let other_reads = function.locals[index + 1..]
            .iter()
            .filter_map(|later| later.initializer.as_ref())
            .map(|later| count_name_occurrences(later, &local.name))
            .sum::<usize>()
            + function
                .guards
                .iter()
                .map(|guard| count_name_occurrences(&guard.value, &local.name))
                .sum::<usize>()
            + function
                .statements
                .iter()
                .map(|statement| statement_reads(statement, &local.name))
                .sum::<usize>()
            + count_name_occurrences(return_expression, &local.name);
        // The pun check runs on the SUBSTITUTED initializer — `int ix = hx & C;`
        // resolves through hx's own punned read first.
        let dedup_safe = is_punned_frame_read(&crate::value_tracking::substitute(initializer, &values)) && other_reads == 0;
        if other_reads + if dedup_safe { 0 } else { guard_condition_reads } > 1 {
            return None;
        }
        let resolved = crate::value_tracking::substitute(initializer, &values);
        values.insert(local.name.clone(), resolved);
    }
    Some(Function {
        return_type: function.return_type,
        section: function.section.clone(),
        asm_body: None, force_active: false,
        name: function.name.clone(),
        is_static: function.is_static,
        is_weak: function.is_weak,
        parameters: function.parameters.clone(),
        locals: Vec::new(),
        statements: function
            .statements
            .iter()
            .map(|statement| substitute_statement(statement, &values))
            .collect(),
        guards: function
            .guards
            .iter()
            .map(|guard| GuardedReturn {
                condition: crate::value_tracking::substitute(&guard.condition, &values),
                value: crate::value_tracking::substitute(&guard.value, &values),
            })
            .collect(),
        return_expression: Some(crate::value_tracking::substitute(return_expression, &values)),
    })
}

/// C89 fdlibm style for the FLOAT paths: a double-returning body whose
/// locals are ALL declared uninitialized and assigned once by LEADING
/// Assign statements normalizes them into initializers (locals reordered to
/// assignment order — the definition order the float tier uses). The guard
/// hoist and this pass alternate through evaluate_body recursion, so
/// `ix = ..; if (..) return x; z = ..;` cleans fully.
pub(crate) fn normalize_leading_local_assigns(function: &Function) -> Option<Function> {
    if function.return_type != Type::Double
        || function.locals.is_empty()
        || function.statements.is_empty()
        || function.locals.iter().any(|local| local.initializer.is_some() || local.array_length.is_some())
    {
        return None;
    }
    let mut assigned: Vec<(String, Expression)> = Vec::new();
    let mut rest = function.statements.as_slice();
    while let [Statement::Assign { name, value }, tail @ ..] = rest {
        let is_declared = function.locals.iter().any(|local| &local.name == name);
        if !is_declared || assigned.iter().any(|(seen, _)| seen == name) || expression_has_call(value) {
            break;
        }
        assigned.push((name.clone(), value.clone()));
        rest = tail;
    }
    if assigned.is_empty() {
        return None;
    }
    // Later statements must not REASSIGN a normalized local (single
    // assignment only).
    let reassigned = rest.iter().any(|statement| {
        matches!(statement, Statement::Assign { name, .. } if assigned.iter().any(|(seen, _)| seen == name))
    });
    if reassigned {
        return None;
    }
    let mut locals: Vec<LocalDeclaration> = Vec::new();
    for (name, value) in &assigned {
        let declared = function.locals.iter().find(|local| &local.name == name).expect("checked above");
        let mut normalized = declared.clone();
        normalized.initializer = Some(value.clone());
        locals.push(normalized);
    }
    for local in &function.locals {
        if !assigned.iter().any(|(name, _)| name == &local.name) {
            locals.push(local.clone());
        }
    }
    Some(Function {
        return_type: function.return_type,
        section: function.section.clone(),
        asm_body: None, force_active: false,
        name: function.name.clone(),
        is_static: function.is_static,
        is_weak: function.is_weak,
        parameters: function.parameters.clone(),
        locals,
        statements: rest.to_vec(),
        guards: function.guards.clone(),
        return_expression: function.return_expression.clone(),
    })
}

pub(crate) fn inline_return_only_locals(function: &Function) -> Option<Function> {
    if function.locals.is_empty() || !function_makes_call(function) || !function.guards.is_empty() {
        return None;
    }
    let return_expression = function.return_expression.as_ref()?;
    // Each local's value, with earlier locals already folded in. A call-bearing
    // initializer is a call result (preserved, not inlined), and a MEMORY-reading one
    // (`int t = arr[i]; g(); return t;` — an array element or global) must load BEFORE
    // the calls it would be carried past (the callee can write that memory) — bail on
    // both so those defer to the callee-saved paths.
    let register_names: std::collections::HashSet<&str> = function
        .parameters
        .iter()
        .map(|parameter| parameter.name.as_str())
        .chain(function.locals.iter().map(|local| local.name.as_str()))
        .collect();
    let mut values: std::collections::HashMap<String, Expression> = std::collections::HashMap::new();
    for local in &function.locals {
        let initializer = local.initializer.as_ref()?;
        if expression_has_call(initializer) || expression_reads_memory(initializer, &register_names) {
            return None;
        }
        let resolved = crate::value_tracking::substitute(initializer, &values);
        values.insert(local.name.clone(), resolved);
    }
    // The locals exist only to feed the return: every statement must be a bare
    // expression that reads none of them (a store/assign/control-flow statement is a
    // different shape).
    for statement in &function.statements {
        let Statement::Expression(expression) = statement else {
            return None;
        };
        if function.locals.iter().any(|local| expression_reads_name(expression, &local.name)) {
            return None;
        }
    }
    Some(Function {
        return_type: function.return_type,
        section: function.section.clone(),
        asm_body: None, force_active: false,
        name: function.name.clone(),
        is_static: function.is_static,
        is_weak: function.is_weak,
        parameters: function.parameters.clone(),
        locals: Vec::new(),
        statements: function.statements.clone(),
        guards: function.guards.clone(),
        return_expression: Some(crate::value_tracking::substitute(return_expression, &values)),
    })
}

/// Inline value-tracked locals that only feed a single `switch` into the switch, then recompile —
/// `int m = n + 1; switch(m) {...}` becomes `switch(n + 1) {...}`, which the switch fast path emits
/// (mwcc compiles them identically). Mirrors `inline_return_only_locals` for a switch body. Each
/// local must be an int-width (>= 32) value with a call-free initializer, read AT MOST ONCE across
/// the scrutinee/arms/default/return, so the substitution cannot duplicate a computation mwcc would
/// keep in a register. Anything outside this leaves the function unchanged (`None`) to defer honestly.
pub(crate) fn inline_switch_scrutinee_locals(function: &Function) -> Option<Function> {
    if function.locals.is_empty() || !function.guards.is_empty() || function_makes_call(function) {
        return None;
    }
    let [Statement::Switch { scrutinee, arms, default }] = function.statements.as_slice() else {
        return None;
    };
    // Each local's value, with earlier locals folded in. A narrow local (width < 32) changes the
    // lowering (truncation/sign-extension) and a call-bearing initializer is a call result — bail.
    let mut values: std::collections::HashMap<String, Expression> = std::collections::HashMap::new();
    for local in &function.locals {
        let initializer = local.initializer.as_ref()?;
        if expression_has_call(initializer) || local.declared_type.width() < 32 {
            return None;
        }
        values.insert(local.name.clone(), crate::value_tracking::substitute(initializer, &values));
    }
    // No inlined local may be read more than once across the whole body, so substituting it cannot
    // duplicate a computation (mwcc materializes a multiply-read value once in a register).
    for local in &function.locals {
        let mut occurrences = crate::analysis::count_name_occurrences(scrutinee, &local.name);
        for arm in arms {
            let Some(result) = arm.result() else {
                return None; // statement-bodied arms skip this fold
            };
            occurrences += crate::analysis::count_name_occurrences(result, &local.name);
        }
        if let Some(body) = default {
            let Some(expression) = body.return_expression() else {
                return None; // a statement-bodied default skips this fold
            };
            occurrences += crate::analysis::count_name_occurrences(expression, &local.name);
        }
        if let Some(expression) = &function.return_expression {
            occurrences += crate::analysis::count_name_occurrences(expression, &local.name);
        }
        if occurrences > 1 {
            return None;
        }
    }
    let arms = arms
        .iter()
        .map(|arm| mwcc_syntax_trees::SwitchArm {
            value: arm.value,
            body: mwcc_syntax_trees::ArmBody::Return(crate::value_tracking::substitute(
                arm.result().expect("gated above"),
                &values,
            )),
            falls_through: false,
        })
        .collect();
    Some(Function {
        return_type: function.return_type,
        section: function.section.clone(),
        asm_body: None, force_active: false,
        name: function.name.clone(),
        is_static: function.is_static,
        is_weak: function.is_weak,
        parameters: function.parameters.clone(),
        locals: Vec::new(),
        statements: vec![Statement::Switch {
            scrutinee: crate::value_tracking::substitute(scrutinee, &values),
            arms,
            default: default.as_ref().map(|body| {
                mwcc_syntax_trees::ArmBody::Return(crate::value_tracking::substitute(
                    body.return_expression().expect("gated above"),
                    &values,
                ))
            }),
        }],
        guards: function.guards.clone(),
        return_expression: function.return_expression.as_ref().map(|expression| crate::value_tracking::substitute(expression, &values)),
    })
}

/// Tally reads of each tracked local in `expression` toward its current value-version's
/// running count, returning true if a computed (non-Variable) version would then be read at
/// a second materialization site. mwcc computes such a value once and keeps it in a
/// register; inlining would duplicate the computation, so the fold must bail. A Variable
/// value is register-resident and free to re-read any number of times.
pub(crate) fn fold_would_duplicate(
    expression: &Expression,
    local_names: &std::collections::HashSet<&str>,
    values: &std::collections::HashMap<String, Expression>,
    read_count: &mut std::collections::HashMap<String, usize>,
) -> bool {
    for &name in local_names {
        let occurrences = crate::analysis::count_name_occurrences(expression, name);
        if occurrences == 0 {
            continue;
        }
        let total = read_count.entry(name.to_string()).or_insert(0);
        *total += occurrences;
        let computed = values.get(name).is_some_and(|value| !matches!(value, Expression::Variable(_)));
        if computed && *total >= 2 {
            return true;
        }
    }
    false
}

/// Fold a function's value-tracked locals into its stores and trailing return, then
/// recompile — `int x = a; gi = x; x = b; gj = x;` becomes `gi = a; gj = b;`, and `int x =
/// a; gi = x; return x;` becomes `gi = a; return a;`. The store paths (or, when mwcc would
/// latency-schedule the stores, the un-schedulable-store deferral) own the cleaned body. The
/// locals exist only to feed the stores and the return, so tracking their values
/// sequentially and substituting eliminates them. Bails on a call (in the body or a value —
/// a side effect to preserve), a guard, a non-store/assign statement, a store into a local,
/// a local that survives the substitution, or a fold that would duplicate a computed value
/// at 2+ sites (mwcc keeps it in one register — fold_would_duplicate). A store-free body
/// (pure dead-local, or pure return-folding) is left to the value-tracking path.
pub(crate) fn inline_store_bearing_locals(function: &Function) -> Option<Function> {
    // Reassigned PARAMETERS fold exactly like locals: `x = x + 1; *p = x;` compiles as
    // `*p = x + 1;` (`addi r0,r4,1; stw r0,0(r3)`) — the store value substitutes the
    // tracked expression, reads before the assignment keep the raw (pristine) register.
    // A narrow reassigned param would drop its re-narrowing when substituted — bail.
    let local_name_set: std::collections::HashSet<&str> =
        function.locals.iter().map(|local| local.name.as_str()).collect();
    let mut reassigned_parameters: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for statement in &function.statements {
        let Statement::Assign { name, .. } = statement else { continue };
        if local_name_set.contains(name.as_str()) {
            continue;
        }
        let Some(parameter) = function.parameters.iter().find(|parameter| &parameter.name == name) else {
            continue;
        };
        if parameter.parameter_type.width() < 32 {
            return None;
        }
        reassigned_parameters.insert(name.as_str());
    }
    if (function.locals.is_empty() && reassigned_parameters.is_empty())
        || function_makes_call(function)
        || !function.guards.is_empty()
    {
        return None;
    }
    // A NARROWING narrow local (`char c = a;` for a wider `a`) must not inline: substituting
    // the wider value drops the truncation + sign-extension — `char c = a; gi = c;` would
    // store the full int instead of `(int)(char)a`. Decline so the function defers honestly
    // on the normal path rather than emitting the raw value.
    let variable_width = |name: &str| -> Option<u32> {
        function
            .parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .map(|parameter| parameter.parameter_type.width() as u32)
            .or_else(|| {
                function
                    .locals
                    .iter()
                    .find(|local| local.name == name)
                    .map(|local| local.declared_type.width() as u32)
            })
    };
    for local in &function.locals {
        if (local.declared_type.width() as u32) < 32 {
            if let Some(Expression::Variable(initializer_name)) = &local.initializer {
                if variable_width(initializer_name).is_some_and(|width| width > local.declared_type.width() as u32) {
                    return None;
                }
            }
        }
    }
    // The TRACKED names: locals plus reassigned parameters. The duplication guard and the
    // substitution treat both alike; the locals-only checks (a store into a local, the
    // must-fully-fold `survives` test) keep `local_name_set` — a parameter legitimately
    // survives in the output (it lives in a register).
    let mut tracked_names = local_name_set.clone();
    tracked_names.extend(reassigned_parameters.iter().copied());
    // Each tracked name's current value, earlier folds applied. Seed from initializers (a
    // call-bearing initializer is a call result to preserve, not inline). `read_count`
    // tracks how many times each name's CURRENT value-version is read, to reject
    // duplicating a computation; reassignment resets it.
    let mut values: std::collections::HashMap<String, Expression> = std::collections::HashMap::new();
    let mut read_count: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for local in &function.locals {
        let Some(initializer) = &local.initializer else { continue };
        if expression_has_call(initializer) || fold_would_duplicate(initializer, &tracked_names, &values, &mut read_count) {
            return None;
        }
        values.insert(local.name.clone(), crate::value_tracking::substitute(initializer, &values));
    }
    let mut new_statements = Vec::new();
    let mut in_leading_ifs = true;
    for statement in &function.statements {
        match statement {
            // A LEADING early-return if passes through unchanged: it executes before any
            // reassignment, so its reads are the pristine registers — correct for a
            // reassigned parameter (its pre-assignment value) — while the substituted
            // stores after it carry their own dataflow. An if reading a LOCAL cannot pass
            // through (the fold removes locals); an if after an assign/store bails.
            Statement::If { condition, then_body, else_body } if in_leading_ifs => {
                if !matches!(then_body.as_slice(), [Statement::Return(_)]) || !else_body.is_empty() {
                    return None;
                }
                let reads_local =
                    |expression: &Expression| local_name_set.iter().any(|name| expression_reads_name(expression, name));
                if reads_local(condition) {
                    return None;
                }
                if let [Statement::Return(Some(value))] = then_body.as_slice() {
                    if reads_local(value) {
                        return None;
                    }
                }
                new_statements.push(statement.clone());
                continue;
            }
            Statement::Assign { name, value } => {
                in_leading_ifs = false;
                if !tracked_names.contains(name.as_str()) || expression_has_call(value) {
                    return None;
                }
                if fold_would_duplicate(value, &tracked_names, &values, &mut read_count) {
                    return None;
                }
                values.insert(name.clone(), crate::value_tracking::substitute(value, &values));
                read_count.insert(name.clone(), 0);
            }
            Statement::Store { target, value } => {
                in_leading_ifs = false;
                if expression_has_call(value) || expression_has_call(target) {
                    return None;
                }
                // A store INTO a local is a different shape — we only fold locals that feed
                // memory stores, not locals that are themselves store targets.
                if let Expression::Variable(name) = target {
                    if local_name_set.contains(name.as_str()) {
                        return None;
                    }
                }
                if fold_would_duplicate(target, &tracked_names, &values, &mut read_count)
                    || fold_would_duplicate(value, &tracked_names, &values, &mut read_count)
                {
                    return None;
                }
                new_statements.push(Statement::Store {
                    target: crate::value_tracking::substitute(target, &values),
                    value: crate::value_tracking::substitute(value, &values),
                });
            }
            _ => return None,
        }
    }
    if let Some(return_expression) = &function.return_expression {
        if fold_would_duplicate(return_expression, &tracked_names, &values, &mut read_count) {
            return None;
        }
    }
    // A store-free body (a pure dead-local, pure return-folding, or a guard prefix with
    // no store behind it) belongs to the value-tracking / guard paths, not ours.
    if !new_statements.iter().any(|statement| matches!(statement, Statement::Store { .. })) {
        return None;
    }
    let folded_return = function
        .return_expression
        .as_ref()
        .map(|expression| crate::value_tracking::substitute(expression, &values));
    // Every local must be fully folded away — none may survive in a resulting store or the
    // return (e.g. a local whose aggregate or address use could not be substituted).
    let survives = |expression: &Expression| local_name_set.iter().any(|name| expression_reads_name(expression, name));
    for statement in &new_statements {
        if let Statement::Store { target, value } = statement {
            if survives(target) || survives(value) {
                return None;
            }
        }
    }
    if folded_return.as_ref().is_some_and(survives) {
        return None;
    }
    Some(Function {
        return_type: function.return_type,
        section: function.section.clone(),
        asm_body: None, force_active: false,
        name: function.name.clone(),
        is_static: function.is_static,
        is_weak: function.is_weak,
        parameters: function.parameters.clone(),
        locals: Vec::new(),
        statements: new_statements,
        guards: function.guards.clone(),
        return_expression: folded_return,
    })
}

/// A single local whose value is exactly one call, consumed once — either stored directly to
/// a global (`int x = foo(...); gi = x;`) or returned (`int x = foo(...); return x;`). mwcc
/// leaves the call result in r3; `gi = foo(...)` and `return foo(...)` are both already
/// byte-exact, and the result is not live across any other call, so it needs no callee-save.
/// Inline the local and recompile. This is the trivial entry into value-tracking-with-calls;
/// a second call, a second use of the result, the result fused with arithmetic, or any other
/// statement all need the callee-saved allocator and fall through to it. (Kept separate from
/// inline_store_bearing_locals, which bails on any call at its entry.)
pub(crate) fn inline_single_call_result(function: &Function) -> Option<Function> {
    if !function.guards.is_empty() || function.locals.len() != 1 {
        return None;
    }
    let local_name = function.locals[0].name.as_str();
    // The local's value is exactly one call, set once — by the initializer xor a single
    // assignment — and the call must not read the local itself.
    let mut call_value: Option<Expression> = None;
    if let Some(initializer) = &function.locals[0].initializer {
        if !matches!(initializer, Expression::Call { .. }) || expression_reads_name(initializer, local_name) {
            return None;
        }
        call_value = Some(initializer.clone());
    }
    let mut store: Option<(Expression, Expression)> = None;
    for statement in &function.statements {
        match statement {
            Statement::Assign { name, value } if name == local_name => {
                if call_value.is_some()
                    || !matches!(value, Expression::Call { .. })
                    || expression_reads_name(value, local_name)
                {
                    return None;
                }
                call_value = Some(value.clone());
            }
            // The result is consumed by exactly one store, whose target does not read the
            // local (only its value may).
            Statement::Store { target, value } => {
                if store.is_some() || expression_reads_name(target, local_name) {
                    return None;
                }
                store = Some((target.clone(), value.clone()));
            }
            _ => return None,
        }
    }
    let call_value = call_value?;
    // The result is consumed in exactly one place and read EXACTLY ONCE there — a call read
    // twice would call twice. Substitute the call into that single use (`gi = x + 1;` ->
    // `gi = foo(a) + 1;`); the re-dispatch is byte-exact (call fused with a constant) or
    // defers (a value live across the call), never a diff.
    let occurrences = |expression: &Expression| crate::analysis::count_name_occurrences(expression, local_name);
    let mut values = std::collections::HashMap::new();
    values.insert(local_name.to_string(), call_value);
    let (statements, return_expression) = match &store {
        // Store sink: a void function with no return, the local consumed once in the value.
        Some((target, value)) if function.return_type == Type::Void && function.return_expression.is_none() => {
            if occurrences(value) != 1 {
                return None;
            }
            (vec![Statement::Store { target: target.clone(), value: crate::value_tracking::substitute(value, &values) }], None)
        }
        // Return sink: no stores, the trailing return consumes the local once.
        None => {
            let return_expression = function.return_expression.as_ref()?;
            if occurrences(return_expression) != 1 {
                return None;
            }
            // If the return also reads a PARAMETER, the call result is combined with a value live
            // ACROSS the call. mwcc keeps the result in its register and the parameter in a callee-
            // saved register, combining in SOURCE order (`int y=f(x); return y+x` -> `add r3,r3,r31`)
            // — different bytes from the inlined call-expression form (`return f(x)+x` -> the callee-
            // saved combine's `add r3,r31,r3`). So do NOT fold it away; leave the local for the
            // callee-saved dispatch (or a clean defer), never a wrong-bytes inline.
            if function.parameters.iter().any(|parameter| expression_reads_name(return_expression, &parameter.name)) {
                return None;
            }
            (Vec::new(), Some(crate::value_tracking::substitute(return_expression, &values)))
        }
        _ => return None,
    };
    Some(Function {
        return_type: function.return_type,
        section: function.section.clone(),
        asm_body: None, force_active: false,
        name: function.name.clone(),
        is_static: function.is_static,
        is_weak: function.is_weak,
        parameters: function.parameters.clone(),
        locals: Vec::new(),
        statements,
        guards: Vec::new(),
        return_expression,
    })
}

/// One arm of a pure-assign select diamond, as a value for the phi register.
pub(crate) enum SelectArm {
    Constant(i16),
    Copy(u8),
    Computed { source: u8, immediate: i16 },
}

/// `*(int*)p` / `*(1+(int*)p)` for a POINTER variable (no AddressOf —
/// the s_modf iptr stores).
pub(crate) fn pointer_word_offset(target: &Expression, pointer: &str) -> Option<i16> {
    let Expression::Dereference { pointer: inner } = target else {
        return None;
    };
    let is_cast = |expression: &Expression| {
        matches!(expression, Expression::Cast { target_type: Type::Pointer(Pointee::Int), operand }
            if matches!(operand.as_ref(), Expression::Variable(name) if name == pointer))
    };
    if is_cast(inner.as_ref()) {
        return Some(0);
    }
    if let Expression::Binary { operator: BinaryOperator::Add, left, right } = inner.as_ref() {
        if crate::analysis::constant_value(left) == Some(1) && is_cast(right) {
            return Some(4);
        }
        if crate::analysis::constant_value(right) == Some(1) && is_cast(left) {
            return Some(4);
        }
    }
    None
}

/// `HUGE + x > 0.0` (the statics folded upstream to literals) — the fdlibm
/// inexact-raising guard, matched at the outer arm level and inside the
/// writeback walker.
pub(crate) fn float_guard_condition(condition: &Expression) -> Option<(u64, u64)> {
    let Expression::Binary { operator: BinaryOperator::Greater, left, right } = condition else {
        return None;
    };
    let Expression::FloatLiteral(zero) = right.as_ref() else {
        return None;
    };
    if *zero != 0.0 {
        return None;
    }
    let Expression::Binary { operator: BinaryOperator::Add, left: huge, right: xvar } = left.as_ref()
    else {
        return None;
    };
    if !matches!(xvar.as_ref(), Expression::Variable(_)) {
        return None;
    }
    let Expression::FloatLiteral(huge) = huge.as_ref() else {
        return None;
    };
    Some((huge.to_bits(), zero.to_bits()))
}

/// The computed guard local `j0 = ((punned >> S) [& M]) - K` shared by the
/// punned-writeback branch path and the zero-select path.
pub(crate) struct GuardLocal<'a> {
    pub(crate) name: &'a str,
    pub(crate) source: &'a str,
    pub(crate) shift: u8,
    pub(crate) mask: Option<i64>,
    pub(crate) offset_k: i64,
}

/// Parse the shift-local initializer `(unsigned)? C >> (guard [- K2])` —
/// the cast selects the LOGICAL shift (srw), the offset folds into the
/// r0 scratch before the shift (arm3's `0xffffffff >> (j0 - 20)`).
pub(crate) fn parse_shift_init(init: &Expression, guard_name: &str) -> Option<(i64, bool, i64)> {
    let Expression::Binary { operator: BinaryOperator::ShiftRight, left, right } = init else {
        return None;
    };
    let (constant_expr, logical) = match left.as_ref() {
        Expression::Cast { target_type: Type::UnsignedInt, operand } => (operand.as_ref(), true),
        other => (other, false),
    };
    let constant = crate::analysis::constant_value(constant_expr)?;
    let (amount, offset) = match right.as_ref() {
        Expression::Binary { operator: BinaryOperator::Subtract, left, right } => {
            let offset = crate::analysis::constant_value(right)?;
            (left.as_ref(), offset)
        }
        other => (other, 0),
    };
    if !matches!(amount, Expression::Variable(v) if v == guard_name) {
        return None;
    }
    Some((constant, logical, offset))
}

/// Parse `((source >> S) [& M]) - K` as a guard-local initializer.
pub(crate) fn parse_guard_init<'a>(name: &'a str, init: &'a Expression) -> Option<GuardLocal<'a>> {
    let (core, offset_k) = match init {
        Expression::Binary { operator: BinaryOperator::Subtract, left, right } => {
            let k = crate::analysis::constant_value(right)?;
            (left.as_ref(), k)
        }
        other => (other, 0),
    };
    let (shifted, mask) = match core {
        Expression::Binary { operator: BinaryOperator::BitAnd, left, right } => {
            let mask = crate::analysis::constant_value(right)?;
            (left.as_ref(), Some(mask))
        }
        other => (other, None),
    };
    let Expression::Binary { operator: BinaryOperator::ShiftRight, left, right } = shifted else {
        return None;
    };
    let Expression::Variable(source) = left.as_ref() else {
        return None;
    };
    let shift = u8::try_from(crate::analysis::constant_value(right)?).ok()?;
    Some(GuardLocal { name, source, shift, mask, offset_k })
}








/// Whether any statement, guard, or the return expression calls one of `names`.
pub(crate) fn function_calls_any(function: &Function, names: &std::collections::HashSet<String>) -> bool {
    fn expression_calls(expression: &Expression, names: &std::collections::HashSet<String>) -> bool {
        use mwcc_syntax_trees::Expression as E;
        match expression {
            E::Call { name, arguments } => {
                names.contains(name) || arguments.iter().any(|argument| expression_calls(argument, names))
            }
            E::Binary { left, right, .. } => expression_calls(left, names) || expression_calls(right, names),
            E::Unary { operand, .. } | E::Cast { operand, .. } | E::AddressOf { operand } => expression_calls(operand, names),
            E::Dereference { pointer } => expression_calls(pointer, names),
            E::Index { base, index } => expression_calls(base, names) || expression_calls(index, names),
            E::Member { base, .. } | E::MemberAddress { base, .. } => expression_calls(base, names),
            E::Conditional { condition, when_true, when_false } => {
                expression_calls(condition, names) || expression_calls(when_true, names) || expression_calls(when_false, names)
            }
            E::Assign { target, value } => expression_calls(target, names) || expression_calls(value, names),
            E::PostStep { target, .. } => expression_calls(target, names),
            E::Comma { left, right } => expression_calls(left, names) || expression_calls(right, names),
            _ => false,
        }
    }
    fn statement_calls(statement: &Statement, names: &std::collections::HashSet<String>) -> bool {
        use mwcc_syntax_trees::Statement as S;
        match statement {
            S::Break | S::Continue | S::Goto(_) | S::Label(_) => false,
            S::Store { target, value } => expression_calls(target, names) || expression_calls(value, names),
            S::Assign { value, .. } => expression_calls(value, names),
            S::Expression(expression) => expression_calls(expression, names),
            S::If { condition, then_body, else_body } => {
                expression_calls(condition, names)
                    || then_body.iter().any(|inner| statement_calls(inner, names))
                    || else_body.iter().any(|inner| statement_calls(inner, names))
            }
            S::Return(value) => value.as_ref().is_some_and(|expression| expression_calls(expression, names)),
            S::Switch { scrutinee, arms, default } => {
                expression_calls(scrutinee, names)
                    || default.as_ref().is_some_and(|body| match body {
                        mwcc_syntax_trees::ArmBody::Return(expression) => expression_calls(expression, names),
                        mwcc_syntax_trees::ArmBody::Statements(statements) => {
                            statements.iter().any(|inner| statement_calls(inner, names))
                        }
                    })
                    || arms.iter().any(|arm| match &arm.body {
                        mwcc_syntax_trees::ArmBody::Return(expression) => expression_calls(expression, names),
                        mwcc_syntax_trees::ArmBody::Statements(statements) => {
                            statements.iter().any(|inner| statement_calls(inner, names))
                        }
                    })
            }
            S::Loop { initializer, condition, step, body, .. } => {
                initializer.as_ref().is_some_and(|expression| expression_calls(expression, names))
                    || condition.as_ref().is_some_and(|expression| expression_calls(expression, names))
                    || step.as_ref().is_some_and(|expression| expression_calls(expression, names))
                    || body.iter().any(|inner| statement_calls(inner, names))
            }
        }
    }
    function.statements.iter().any(|statement| statement_calls(statement, names))
        || function.guards.iter().any(|guard| {
            expression_calls(&guard.condition, names) || expression_calls(&guard.value, names)
        })
        || function
            .return_expression
            .as_ref()
            .is_some_and(|expression| expression_calls(expression, names))
}





