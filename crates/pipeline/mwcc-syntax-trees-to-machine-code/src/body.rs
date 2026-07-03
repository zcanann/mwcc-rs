//! Function-level emission: parameters, body, guards, and the return tail.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{Instruction, RelocationKind};
use mwcc_syntax_trees::{BinaryOperator, Expression, Function, GuardedReturn, LocalDeclaration, LoopKind, Pointee, Statement, Type, UnaryOperator};
use mwcc_versions::GlobalAddressing;
use crate::expressions::{displacement_store, pointee_of_type};

/// How a run of constant stores materializes its values (see `constant_store_run_plan`). `AllSame`
/// reuses the scratch register for one repeated `li`; `Distinct` gives each store's value its own
/// register (materialized up front, r(N+1) descending to r3 with the last in r0), stored in source
/// order.
enum ConstStoreRun {
    AllSame,
    Distinct(Vec<(i32, u8)>),
}

/// The `(operand, constant)` a guard condition compares against, when it is `<var> OP <const>`
/// (or the commuted `<const> OP <var>`). Two consecutive guards with the same key share one
/// `cmpwi` in mwcc, which emit_guard_sequence does not model (so it defers such a pair).
fn guard_comparison_key(condition: &Expression) -> Option<(String, i64)> {
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
fn accesses_pointer(expression: &Expression, pointer: &str) -> bool {
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
fn guarded_null_dereference<'a>(condition: &'a Expression, value: &'a Expression, default: &'a Expression, return_type: Type) -> Option<(&'a str, &'a Expression, &'a Expression)> {
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
fn guard_select(condition: &Expression, value: &Expression, default: &Expression) -> Expression {
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
use mwcc_target::Eabi;
use crate::analysis::*;
use crate::expressions::pointer_stride;
use crate::generator::*;

/// Whether a statement references (reads, writes, or takes the address of) `name`.
/// Control-flow statements are treated conservatively as referencing everything.
fn statement_references_name(statement: &Statement, name: &str) -> bool {
    match statement {
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
fn remove_dead_locals(function: &Function) -> Option<Function> {
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

/// Fold a pure function-pointer alias local into the single call THROUGH it: `F t = gf;
/// t();` compiles exactly like `gf();` (mwcc loads the pointer right before `mtctr`
/// either way — the load position is unchanged). Only the exactly-safe shape folds: the
/// alias's ONLY use is as the call target of the FIRST statement (a later call-through
/// would observe a possibly-rewritten global; a read anywhere else needs the register
/// allocation the fold erases).
fn inline_first_call_target_alias(function: &Function) -> Option<Function> {
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
fn substitute_statement(statement: &Statement, values: &std::collections::HashMap<String, Expression>) -> Statement {
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

fn statement_reads(statement: &Statement, name: &str) -> usize {
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
fn is_punned_frame_read(expression: &Expression) -> bool {
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

fn inline_frame_feeding_locals(function: &Function) -> Option<Function> {
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
fn normalize_leading_local_assigns(function: &Function) -> Option<Function> {
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

fn inline_return_only_locals(function: &Function) -> Option<Function> {
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
fn inline_switch_scrutinee_locals(function: &Function) -> Option<Function> {
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
        if let Some(expression) = default {
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
        })
        .collect();
    Some(Function {
        return_type: function.return_type,
        name: function.name.clone(),
        is_static: function.is_static,
        is_weak: function.is_weak,
        parameters: function.parameters.clone(),
        locals: Vec::new(),
        statements: vec![Statement::Switch {
            scrutinee: crate::value_tracking::substitute(scrutinee, &values),
            arms,
            default: default.as_ref().map(|expression| crate::value_tracking::substitute(expression, &values)),
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
fn fold_would_duplicate(
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
fn inline_store_bearing_locals(function: &Function) -> Option<Function> {
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
fn inline_single_call_result(function: &Function) -> Option<Function> {
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
enum SelectArm {
    Constant(i16),
    Copy(u8),
    Computed { source: u8, immediate: i16 },
}

/// `*(int*)p` / `*(1+(int*)p)` for a POINTER variable (no AddressOf —
/// the s_modf iptr stores).
fn pointer_word_offset(target: &Expression, pointer: &str) -> Option<i16> {
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
fn float_guard_condition(condition: &Expression) -> Option<(u64, u64)> {
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
struct GuardLocal<'a> {
    name: &'a str,
    source: &'a str,
    shift: u8,
    mask: Option<i64>,
    offset_k: i64,
}

/// Parse the shift-local initializer `(unsigned)? C >> (guard [- K2])` —
/// the cast selects the LOGICAL shift (srw), the offset folds into the
/// r0 scratch before the shift (arm3's `0xffffffff >> (j0 - 20)`).
fn parse_shift_init(init: &Expression, guard_name: &str) -> Option<(i64, bool, i64)> {
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
fn parse_guard_init<'a>(name: &'a str, init: &'a Expression) -> Option<GuardLocal<'a>> {
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

impl Generator {

    pub(crate) fn assign_parameters(&mut self, function: &Function) -> Compilation<()> {
        self.known_locals = function.locals.iter().map(|local| local.name.clone()).collect();
        let mut next_general = Eabi::FIRST_GENERAL_ARGUMENT;
        let mut next_float = Eabi::FIRST_FLOAT_ARGUMENT;
        for parameter in &function.parameters {
            let class = class_of(parameter.parameter_type)?;
            let register = match class {
                ValueClass::General => {
                    let register = next_general;
                    next_general += 1;
                    register
                }
                ValueClass::Float => {
                    let register = next_float;
                    next_float += 1;
                    register
                }
            };
            let signed = self.signed_of(parameter.parameter_type);
            let pointee = match parameter.parameter_type {
                Type::Pointer(pointee) => Some(pointee),
                _ => None,
            };
            let stride = pointer_stride(parameter.parameter_type);
            self.locations.insert(
                parameter.name.clone(),
                Location { class, register, signed, width: parameter.parameter_type.width(), pointee, stride },
            );
        }
        Ok(())
    }

    /// Emit a function involving a long long (64-bit) value, held in a general-register PAIR
    /// (`r3:r4` = high:low). Only a narrow set of shapes is modeled; the rest defer rather than
    /// fall through to the 32-bit codegen (which emits a single-register result for a 64-bit value).
    fn emit_long_long(&mut self, function: &Function) -> Compilation<()> {
        // Long-long LOCALS (which need pair spills), guards, and statements are not modeled yet.
        if !function.locals.is_empty() || !function.guards.is_empty() || !function.statements.is_empty() {
            return Err(Diagnostic::error("this long long shape is not modeled yet (roadmap)"));
        }
        let high = Eabi::general_result().number; // r3 — the result HIGH word
        let low = high + 1; //                       r4 — the result LOW word
        let return_expression = function
            .return_expression
            .as_ref()
            .ok_or_else(|| Diagnostic::error("a non-void long long function needs a return value"))?;
        let any_long_long_parameter = function
            .parameters
            .iter()
            .any(|parameter| matches!(parameter.parameter_type, Type::LongLong | Type::UnsignedLongLong));

        // ===== No long-long PARAMETERS: a long-long RETURN from a constant or a widened 32-bit value.
        if !any_long_long_parameter {
            if !matches!(function.return_type, Type::LongLong | Type::UnsignedLongLong) {
                return Err(Diagnostic::error("this long long shape is not modeled yet (roadmap)"));
            }
            // (a) A 64-bit integer CONSTANT — `li low,LOW ; li high,HIGH` (LOW word first, as mwcc
            // emits it). Restricted to words that load with a single `li`.
            if let Some(value) = crate::analysis::constant_value(return_expression) {
                let low_word = value as i32 as i64;
                let high_word = value >> 32;
                if i16::try_from(low_word).is_err() || i16::try_from(high_word).is_err() {
                    return Err(Diagnostic::error("a wide long long constant needs lis/ori (roadmap)"));
                }
                self.load_integer_constant(low, low_word);
                self.load_integer_constant(high, high_word);
                self.emit_epilogue_and_return();
                return Ok(());
            }
            // (b) Widen a 32-bit int/unsigned FIRST PARAMETER. It arrives in r3 (= result HIGH), so
            // copy it to LOW, then fill HIGH with its sign (`srawi`) or zero (`li`). A NARROW source
            // (short/char) re-extends differently and defers.
            if let Expression::Variable(name) = return_expression {
                if function.parameters.first().is_some_and(|parameter| &parameter.name == name) {
                    let parameter_type = function.parameters[0].parameter_type;
                    if matches!(parameter_type, Type::Int | Type::UnsignedInt) {
                        self.output.instructions.push(Instruction::move_register(low, high));
                        if parameter_type.is_signed() {
                            self.output
                                .instructions
                                .push(Instruction::ShiftRightAlgebraicImmediate { a: high, s: high, shift: 31 });
                        } else {
                            self.load_integer_constant(high, 0);
                        }
                        self.emit_epilogue_and_return();
                        return Ok(());
                    }
                }
            }
            return Err(Diagnostic::error("this long long return shape is not modeled yet (roadmap)"));
        }

        // ===== Long-long PARAMETERS present. Allocate GPR argument registers per the EABI: each
        // int-like param takes one GPR; each long-long param an odd-start GPR pair (aligning up if
        // the next GPR is even), so `f(int x, long long a)` puts x in r3 and a in r5:r6. A float/
        // double/struct param alongside a long long (FPRs or aggregates) and an argument list that
        // overflows r3..r10 both defer.
        const LAST_GENERAL_ARGUMENT: u8 = Eabi::FIRST_GENERAL_ARGUMENT + 7; // r10
        let mut next_general = Eabi::FIRST_GENERAL_ARGUMENT;
        let mut param_pair: std::collections::HashMap<&str, (u8, Option<u8>)> = std::collections::HashMap::new();
        for parameter in &function.parameters {
            match parameter.parameter_type {
                Type::LongLong | Type::UnsignedLongLong => {
                    if next_general % 2 == 0 {
                        next_general += 1; // a long-long pair starts on an odd register
                    }
                    if next_general + 1 > LAST_GENERAL_ARGUMENT {
                        return Err(Diagnostic::error("a long-long argument that overflows to the stack is not modeled yet (roadmap)"));
                    }
                    param_pair.insert(parameter.name.as_str(), (next_general, Some(next_general + 1)));
                    next_general += 2;
                }
                Type::Int | Type::UnsignedInt | Type::Short | Type::UnsignedShort | Type::Char | Type::UnsignedChar
                | Type::Pointer(_) | Type::StructPointer { .. } => {
                    if next_general > LAST_GENERAL_ARGUMENT {
                        return Err(Diagnostic::error("an integer argument that overflows to the stack is not modeled yet (roadmap)"));
                    }
                    param_pair.insert(parameter.name.as_str(), (next_general, None));
                    next_general += 1;
                }
                _ => return Err(Diagnostic::error("a float/double/struct parameter alongside a long long is not modeled yet (roadmap)")),
            }
        }

        // (c) TRUNCATE a long-long param to int/unsigned — `(int)a` or implicit — is its LOW word:
        // `mr r3, low(a)`.
        if matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            let truncated = match return_expression {
                Expression::Cast { target_type: Type::Int | Type::UnsignedInt, operand } => operand.as_ref(),
                other => other,
            };
            if let Expression::Variable(name) = truncated {
                if let Some(&(_, Some(low_register))) = param_pair.get(name.as_str()) {
                    self.output.instructions.push(Instruction::move_register(high, low_register));
                    self.emit_epilogue_and_return();
                    return Ok(());
                }
            }
            return Err(Diagnostic::error("this long long truncation is not modeled yet (roadmap)"));
        }
        if !matches!(function.return_type, Type::LongLong | Type::UnsignedLongLong) {
            return Err(Diagnostic::error("this long long shape is not modeled yet (roadmap)"));
        }

        // (d) RETURN a long-long param: move its pair into the result pair (a bare `blr` when it is
        // already there — the first parameter). mwcc moves LOW then HIGH (`mr r4,r6 ; mr r3,r5`).
        if let Expression::Variable(name) = return_expression {
            if let Some(&(parameter_high, Some(parameter_low))) = param_pair.get(name.as_str()) {
                if parameter_high != high {
                    self.output.instructions.push(Instruction::move_register(low, parameter_low));
                    self.output.instructions.push(Instruction::move_register(high, parameter_high));
                }
                self.emit_epilogue_and_return();
                return Ok(());
            }
        }

        // (e) ADD / SUBTRACT two long-long params into the result pair; the LOW word carries into
        // HIGH: `addc r4,r4,r6 ; adde r3,r3,r5` or `subfc r4,r6,r4 ; subfe r3,r5,r3`.
        if let Expression::Binary { operator, left, right } = return_expression {
            if let (Expression::Variable(left_name), Expression::Variable(right_name)) = (left.as_ref(), right.as_ref()) {
                if let (Some(&(left_high, Some(left_low))), Some(&(right_high, Some(right_low)))) =
                    (param_pair.get(left_name.as_str()), param_pair.get(right_name.as_str()))
                {
                    match operator {
                        BinaryOperator::Add => {
                            self.output.instructions.push(Instruction::AddCarrying { d: low, a: left_low, b: right_low });
                            self.output.instructions.push(Instruction::AddExtended { d: high, a: left_high, b: right_high });
                            self.emit_epilogue_and_return();
                            return Ok(());
                        }
                        // subfc rD,rA,rB = rB - rA, so the minuend (left) is `b` and subtrahend (right) is `a`.
                        BinaryOperator::Subtract => {
                            self.output.instructions.push(Instruction::SubtractFromCarrying { d: low, a: right_low, b: left_low });
                            self.output.instructions.push(Instruction::SubtractFromExtended { d: high, a: right_high, b: left_high });
                            self.emit_epilogue_and_return();
                            return Ok(());
                        }
                        _ => {}
                    }
                }
            }
        }

        // (f) ADD/SUBTRACT a small CONSTANT to a single long-long parameter. mwcc materializes the
        // 64-bit constant — its LOW word into the next free GPR (r5) and its HIGH word into r0, or
        // just r0 when both words are equal — then `addc`/`adde`. `a - C` lowers as `a + (-C)`.
        // Restricted to a single long-long parameter (so a == result == r3:r4 and r5 is free) and
        // li-sized constant words; a wider constant or a second parameter (dead-register reuse)
        // defers.
        if function.parameters.len() == 1 {
            if let Expression::Binary { operator, left, right } = return_expression {
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract) {
                    if let (Expression::Variable(name), Some(constant)) = (left.as_ref(), crate::analysis::constant_value(right)) {
                        if param_pair.get(name.as_str()).is_some_and(|&(_, low_word)| low_word.is_some()) {
                            let value = if *operator == BinaryOperator::Subtract { constant.wrapping_neg() } else { constant };
                            let low_word = value as i32 as i64;
                            let high_word = value >> 32;
                            if i16::try_from(low_word).is_ok() && i16::try_from(high_word).is_ok() {
                                if low_word == high_word {
                                    self.load_integer_constant(GENERAL_SCRATCH, low_word);
                                    self.output.instructions.push(Instruction::AddCarrying { d: low, a: low, b: GENERAL_SCRATCH });
                                    self.output.instructions.push(Instruction::AddExtended { d: high, a: high, b: GENERAL_SCRATCH });
                                } else {
                                    let low_constant_register = high + 2; // r5 — the next free GPR after r3:r4
                                    self.load_integer_constant(low_constant_register, low_word);
                                    self.load_integer_constant(GENERAL_SCRATCH, high_word);
                                    self.output.instructions.push(Instruction::AddCarrying { d: low, a: low, b: low_constant_register });
                                    self.output.instructions.push(Instruction::AddExtended { d: high, a: high, b: GENERAL_SCRATCH });
                                }
                                self.emit_epilogue_and_return();
                                return Ok(());
                            }
                        }
                    }
                }
            }
        }

        Err(Diagnostic::error("this long long shape is not modeled yet (roadmap)"))
    }

    /// Emit the whole function body, including its `blr`(s).
    /// A body that continued past its early-return guards parses them into the ordered
    /// statement list as `If { then_body: [Return(Some(v))] }` entries. When every such
    /// leading guard reads only names the remaining statements never assign (and no local,
    /// whose tracked value the guard would need substituted), the guard reads the same
    /// pristine registers whether emitted before or after the (virtual, value-tracked)
    /// reassignments — so it moves back into `guards` for the trailing-guard machinery.
    /// Only shapes where mwcc compiles both orders IDENTICALLY hoist: the guard value must
    /// be a CONSTANT (a register value branches in the ordered source but folds inverted in
    /// the flat one) and the tail must not read the result register's parameter (the fold's
    /// `li r3,V` clobbers it — mwcc branches in the ordered source, folds through a temp in
    /// the flat one). The rest must be pure reassignments (the value-tracking shape).
    fn hoist_order_independent_leading_guards(&self, function: &Function) -> Option<Function> {
        if !matches!(function.statements.first(), Some(Statement::If { .. })) {
            return None;
        }
        let mut hoisted: Vec<GuardedReturn> = Vec::new();
        let mut rest: Vec<Statement> = Vec::new();
        let mut in_prefix = true;
        for statement in &function.statements {
            if in_prefix {
                if let Statement::If { condition, then_body, else_body } = statement {
                    if else_body.is_empty() {
                        if let [Statement::Return(Some(value))] = then_body.as_slice() {
                            hoisted.push(GuardedReturn { condition: condition.clone(), value: value.clone() });
                            continue;
                        }
                    }
                }
                in_prefix = false;
            }
            rest.push(statement.clone());
        }
        if hoisted.is_empty() || !rest.iter().all(|statement| matches!(statement, Statement::Assign { .. })) {
            return None;
        }
        let written: Vec<&str> = rest
            .iter()
            .filter_map(|statement| match statement {
                Statement::Assign { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .chain(function.locals.iter().map(|local| local.name.as_str()))
            .collect();
        let reads_written = |expression: &Expression| written.iter().any(|name| expression_reads_name(expression, name));
        if hoisted.iter().any(|guard| reads_written(&guard.condition) || reads_written(&guard.value)) {
            return None;
        }
        // A guard value must be a constant (the direct fold) or a plain variable (the
        // inverted fold: `cmpwi; addi r3,r4,1; beqlr; mr r3,c` — verified identical in
        // both orders for one-parameter tails). Computed values stay ordered.
        if hoisted
            .iter()
            .any(|guard| constant_value(&guard.value).is_none() && !matches!(&guard.value, Expression::Variable(_)))
        {
            return None;
        }
        // The tail (any reassigned value, or the return expression) must not read the
        // parameter living in the result register — the fold clobbers it. Such bodies
        // stay ordered for the branch-form handler.
        if let Some(occupant) = self.locations.iter().find_map(|(name, location)| {
            (location.register == mwcc_target::Eabi::general_result().number && location.class == ValueClass::General)
                .then_some(name.as_str())
        }) {
            let tail_reads_occupant = rest.iter().any(|statement| match statement {
                Statement::Assign { value, .. } => expression_reads_name(value, occupant),
                _ => false,
            }) || function.return_expression.as_ref().is_some_and(|ret| expression_reads_name(ret, occupant));
            if tail_reads_occupant {
                return None;
            }
        }
        // A tail reading TWO OR MORE distinct parameters does not fold directly either:
        // mwcc schedules it into the local's home register ahead of the guard value
        // (`add r0,r4,r5; li r3,5; bnelr; mr r3,r0` flat, a real branch ordered) — an
        // order-dependent form, so it too stays ordered for the branch-form handler.
        let tail_reads_parameter = |name: &str| {
            rest.iter().any(|statement| match statement {
                Statement::Assign { value, .. } => expression_reads_name(value, name),
                _ => false,
            }) || function.return_expression.as_ref().is_some_and(|ret| expression_reads_name(ret, name))
        };
        if function.parameters.iter().filter(|parameter| tail_reads_parameter(&parameter.name)).count() > 1 {
            return None;
        }
        let mut guards = hoisted;
        guards.extend(function.guards.iter().cloned());
        Some(Function { statements: rest, guards, ..function.clone() })
    }

    /// The ordered early-return BRANCH form: a single leading `if (c) return v;` whose body
    /// continues with pure reassignments. Where the constant fold does not apply (a register
    /// guard value, or a tail still reading the result register's parameter), mwcc emits a
    /// real forward branch — `<condition>; b<false> CONT; <value into r3>; blr; CONT: <tail>`
    /// (`if (a) return c; b = b + c; return b;` → `cmpwi; beq +8; mr r3,r5; blr; add; blr`).
    /// The guard must read only names the rest never assigns (a guard reading an assigned
    /// name joins through r0 instead — not modeled). The continuation is delegated to value
    /// tracking; a continuation it cannot compile defers the whole body (the guard block is
    /// already emitted, so a bare `Ok(false)` would leave partial output).
    /// A guarded computed-index GLOBAL-ARRAY store with a constant return:
    /// `if (i < 1) return -1; arr[i - 1] = 0; return 0;` (the signal.c shape). The
    /// address build interleaves with the live return value, in three captured forms:
    /// - constant value, offset 0:  `lis r4; slwi r0,i; addi r3,r4; li r4,C; stwx r4,r3,r0; li r3,R`
    /// - constant value, offset ±k: `lis r4; slwi; addi r3,r4; li r5,C; add r4,r3,r0; li r3,R; stw r5,k(r4)`
    /// - register value, offset 0:  `lis r5; slwi; addi r5,r5; li r3,R; stwx v,r5,r0`
    /// A register value with a folded offset is uncaptured; small (SDA21) arrays,
    /// float/byte elements, and non-constant returns fall to the scheduler defer.
    #[allow(clippy::too_many_arguments)]
    fn try_guarded_global_array_store(
        &mut self,
        function: &Function,
        condition: &Expression,
        guard_value: &Expression,
        array: &str,
        total_size: u32,
        index: &Expression,
        stored: &Expression,
    ) -> Compilation<bool> {
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let Some(return_constant) = function
            .return_expression
            .as_ref()
            .and_then(|expression| constant_value(expression))
            .and_then(|constant| i16::try_from(constant).ok())
        else {
            return Ok(false);
        };
        if self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8 {
            return Ok(false);
        }
        let Some(pointee) = pointee_of_type(self.globals[array]) else {
            return Ok(false);
        };
        if matches!(pointee, Pointee::Float | Pointee::Double) {
            return Ok(false);
        }
        let size = pointee.size();
        if size == 1 {
            return Ok(false);
        }
        // `arr[i ± k]` folds the scaled element offset onto the store displacement.
        let mut index_leaf = index;
        let mut element_offset: i64 = 0;
        if let Expression::Binary { operator, left, right } = index {
            if let Some(k) = constant_value(right) {
                match operator {
                    BinaryOperator::Add => {
                        index_leaf = left.as_ref();
                        element_offset = k * size as i64;
                    }
                    BinaryOperator::Subtract => {
                        index_leaf = left.as_ref();
                        element_offset = -k * size as i64;
                    }
                    _ => {}
                }
            }
        }
        if !matches!(index_leaf, Expression::Variable(_)) {
            return Ok(false);
        }
        let Ok(offset) = i16::try_from(element_offset) else {
            return Ok(false);
        };
        let stored_constant = constant_value(stored).and_then(|constant| i16::try_from(constant).ok());
        let stored_register = if stored_constant.is_none() {
            let Expression::Variable(name) = stored else { return Ok(false) };
            let Some(register) = self.lookup_general(name) else { return Ok(false) };
            if offset != 0 {
                return Ok(false);
            }
            Some(register)
        } else {
            None
        };

        let result = mwcc_target::Eabi::general_result().number;
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let branch_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
        self.evaluate_tail(guard_value, function.return_type, result)?;
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        let continuation = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
            *target = continuation;
        }

        let index_register = self.general_register_of_leaf(index_leaf)?;
        let shift = size.trailing_zeros() as u8;
        if let Some(register) = stored_register {
            // Register value: the base stays OUT of the index register (the return needs
            // r3 live before the store) — `lis B; slwi; addi B,B; li r3,R; stwx v,B,r0`.
            let base = self.fresh_virtual_general();
            self.emit_address_high(base, array);
            self.output.instructions.push(Instruction::ShiftLeftImmediate { a: GENERAL_SCRATCH, s: index_register, shift });
            self.record_relocation(RelocationKind::Addr16Lo, array);
            self.output.instructions.push(Instruction::AddImmediate { d: base, a: base, immediate: 0 });
            self.output.instructions.push(Instruction::AddImmediate { d: result, a: 0, immediate: return_constant });
            self.output.instructions.push(crate::expressions::indexed_store(pointee, register, base, GENERAL_SCRATCH));
        } else {
            let constant = stored_constant.expect("checked above");
            // Phase D: the base-high is a virtual in both forms — a redefined vreg keeps
            // ONE live range spanning the redefinition (the offset≠0 form reuses it for
            // the effective address), so the value's overlapping virtual lands on the
            // next register, matching mwcc's r4/r5 split.
            let high = self.fresh_virtual_general();
            self.emit_address_high(high, array);
            self.output.instructions.push(Instruction::ShiftLeftImmediate { a: GENERAL_SCRATCH, s: index_register, shift });
            self.record_relocation(RelocationKind::Addr16Lo, array);
            self.output.instructions.push(Instruction::AddImmediate { d: index_register, a: high, immediate: 0 });
            if offset == 0 {
                // The standalone sequence, the return materialized after the store.
                self.output.instructions.push(Instruction::AddImmediate { d: high, a: 0, immediate: constant });
                self.output.instructions.push(crate::expressions::indexed_store(pointee, high, index_register, GENERAL_SCRATCH));
                self.output.instructions.push(Instruction::AddImmediate { d: result, a: 0, immediate: return_constant });
            } else {
                // The value's virtual overlaps the still-live high (which the `add`
                // redefines as the effective address), so it allocates past it.
                let value_register = self.fresh_virtual_general();
                self.output.instructions.push(Instruction::AddImmediate { d: value_register, a: 0, immediate: constant });
                self.output.instructions.push(Instruction::Add { d: high, a: index_register, b: GENERAL_SCRATCH });
                self.output.instructions.push(Instruction::AddImmediate { d: result, a: 0, immediate: return_constant });
                self.output.instructions.push(displacement_store(pointee, value_register, high, offset));
            }
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    fn try_ordered_early_return_branch(&mut self, function: &Function) -> Compilation<bool> {
        // A VOID early return over a single-store continuation: `if (a) return; *p = 5;`
        // is a conditional RETURN (the void exit needs no value), then the plain store
        // body — `cmpwi; bnelr; li r0,5; stw r0,0(r4); blr`. The store emission is the
        // standalone sequential form (no return value to schedule around).
        if function.return_type == Type::Void
            && function.guards.is_empty()
            && function.return_expression.is_none()
            && function.locals.is_empty()
            && !function_makes_call(function)
        {
            if let [Statement::If { condition, then_body, else_body }, rest @ ..] = function.statements.as_slice() {
                if matches!(then_body.as_slice(), [Statement::Return(None)])
                    && else_body.is_empty()
                    && matches!(rest, [Statement::Store { .. }])
                {
                    let (options, condition_bit) = self.emit_condition_test(condition)?;
                    self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: options ^ 8, condition_bit });
                    self.emit_statement(&rest[0])?;
                    self.emit_epilogue_and_return();
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        if !function.guards.is_empty() || function.return_type == Type::Void || function.return_expression.is_none() {
            return Ok(false);
        }
        let [Statement::If { condition, then_body, else_body }, rest @ ..] = function.statements.as_slice() else {
            return Ok(false);
        };
        let [Statement::Return(Some(value))] = then_body.as_slice() else {
            return Ok(false);
        };
        if !else_body.is_empty() || rest.len() != 1 || function_makes_call(function) {
            return Ok(false);
        }

        // A store continuation: `if (a) return -1; *p = 5; return 0;`. A MATERIALIZED store
        // value (a constant, or a simple two-leaf computation) lands in r0 with the return
        // value scheduled BETWEEN the materialization and the store — `li r0,5; li r3,0;
        // stw r0,0(r4); blr` / `addi r0,r5,1; li r3,0; stw r0,0(r4)` (or `mr r3,x` for a
        // register return). Covers `*p`, `p[const]`, and `p->member` targets. A register-
        // valued store needs no materialization and stays with the sequential path (store,
        // then the return move — verified byte-exact there); two or more stores interleave
        // through the batch scheduler and defer.
        if let [Statement::Store { target, value: stored }] = rest {
            if function.guards.is_empty() && function.locals.is_empty() {
                // A computed-index GLOBAL-ARRAY target has its own captured schedules
                // (the address build interleaves with the return) — a dedicated arm.
                if let Expression::Index { base, index } = target {
                    if let Expression::Variable(array) = base.as_ref() {
                        if let Some(&total_size) = self.global_array_sizes.get(array.as_str()) {
                            if constant_value(index).is_none() {
                                return self.try_guarded_global_array_store(function, condition, value, array, total_size, index, stored);
                            }
                        }
                    }
                }
                let stored_is_constant = constant_value(stored).and_then(|constant| i16::try_from(constant).ok()).is_some();
                let stored_is_two_leaf = matches!(stored, Expression::Binary { left, right, .. }
                    if matches!(left.as_ref(), Expression::Variable(_) | Expression::IntegerLiteral(_))
                        && matches!(right.as_ref(), Expression::Variable(_) | Expression::IntegerLiteral(_)));
                if !stored_is_constant && !stored_is_two_leaf {
                    return Ok(false);
                }
                let (pointer_name, byte_offset, pointee): (&String, i64, Pointee) = match target {
                    Expression::Dereference { pointer } => {
                        let Expression::Variable(name) = pointer.as_ref() else { return Ok(false) };
                        (name, 0, self.pointee_of(pointer)?)
                    }
                    Expression::Index { base, index } => {
                        let Expression::Variable(name) = base.as_ref() else { return Ok(false) };
                        let Some(constant) = constant_value(index) else { return Ok(false) };
                        let pointee = self.pointee_of(base)?;
                        (name, constant * pointee.size() as i64, pointee)
                    }
                    Expression::Member { base, offset, member_type, index_stride: None } => {
                        let Expression::Variable(name) = base.as_ref() else { return Ok(false) };
                        let Some(pointee) = pointee_of_type(*member_type) else { return Ok(false) };
                        (name, *offset as i64, pointee)
                    }
                    _ => return Ok(false),
                };
                if !function.parameters.iter().any(|parameter| &parameter.name == pointer_name) {
                    return Ok(false);
                }
                let Some(pointer_register) = self.lookup_general(pointer_name) else {
                    return Ok(false);
                };
                if matches!(pointee, Pointee::Float | Pointee::Double) {
                    return Ok(false);
                }
                let Ok(offset) = i16::try_from(byte_offset) else {
                    return Ok(false);
                };
                // The return value: a constant `li`, or a General register `mr`.
                enum ReturnValue {
                    Constant(i16),
                    Register(u8),
                }
                let return_value = match function.return_expression.as_ref() {
                    Some(expression) => {
                        if let Some(constant) = constant_value(expression).and_then(|constant| i16::try_from(constant).ok()) {
                            ReturnValue::Constant(constant)
                        } else if let Expression::Variable(name) = expression {
                            match self.lookup_general(name) {
                                Some(register) => ReturnValue::Register(register),
                                None => return Ok(false),
                            }
                        } else {
                            return Ok(false);
                        }
                    }
                    None => return Ok(false),
                };
                if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
                    return Ok(false);
                }

                let result = mwcc_target::Eabi::general_result().number;
                let (options, condition_bit) = self.emit_condition_test(condition)?;
                let branch_index = self.output.instructions.len();
                self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
                self.evaluate_tail(value, function.return_type, result)?;
                self.output.instructions.push(Instruction::BranchToLinkRegister);
                let continuation = self.output.instructions.len();
                if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                    *target = continuation;
                }
                self.evaluate_general(stored, GENERAL_SCRATCH)?;
                match return_value {
                    ReturnValue::Constant(constant) => {
                        self.output.instructions.push(Instruction::AddImmediate { d: result, a: 0, immediate: constant });
                    }
                    ReturnValue::Register(register) => {
                        self.output.instructions.push(Instruction::Or { a: result, s: register, b: register });
                    }
                }
                self.output.instructions.push(displacement_store(pointee, GENERAL_SCRATCH, pointer_register, offset));
                self.emit_epilogue_and_return();
                return Ok(true);
            }
            return Ok(false);
        }

        // A single reassignment is the verified continuation shape; longer tails are
        // unverified against mwcc (they may fold or reschedule differently) — defer.
        if !rest.iter().all(|statement| matches!(statement, Statement::Assign { .. })) {
            return Ok(false);
        }
        let written: Vec<&str> = rest
            .iter()
            .filter_map(|statement| match statement {
                Statement::Assign { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .chain(function.locals.iter().map(|local| local.name.as_str()))
            .collect();
        let reads_written = |expression: &Expression| written.iter().any(|name| expression_reads_name(expression, name));
        // A guard VALUE reading a reassigned name is unverified — defer.
        if reads_written(value) {
            return Ok(false);
        }
        let tail_reads_parameter = |name: &str| {
            rest.iter().any(|statement| match statement {
                Statement::Assign { value, .. } => expression_reads_name(value, name),
                _ => false,
            }) || function.return_expression.as_ref().is_some_and(|ret| expression_reads_name(ret, name))
        };
        let distinct_parameter_reads =
            function.parameters.iter().filter(|parameter| tail_reads_parameter(&parameter.name)).count();

        // The branch form is mwcc's shape for a tail reading TWO-plus distinct parameters
        // (`add r3,r4,r5` after the branch), with a condition reading no reassigned name.
        if distinct_parameter_reads >= 2 && !reads_written(condition) {
            // The guard block: branch past it when the condition is false, else the value and
            // return. emit_condition_test yields the skip-when-false encoding directly.
            let result = mwcc_target::Eabi::general_result().number;
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            let branch_index = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
            self.evaluate_tail(value, function.return_type, result)?;
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            let continuation = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                *target = continuation;
            }

            // The continuation is a pure value-tracking body; anything it cannot compile must
            // DEFER (the guard block is already in the output).
            let reduced = Function { statements: rest.to_vec(), ..function.clone() };
            if !self.try_value_tracking(&reduced)? {
                return Err(Diagnostic::error("an early-return continuation outside the value-tracking shape is not supported yet (roadmap)"));
            }
            return Ok(true);
        }

        // A ONE-parameter tail with a register guard value takes the INVERTED FOLD even when
        // the condition reads the reassigned name — the compare tests the ORIGINAL value
        // before the tail clobbers it in place: `if (a) return b; a = a + 1; return a;` →
        // `cmpwi r3,0; addi r3,r3,1; beqlr; mr r3,r4`. Kept to the exactly-verified shape: a
        // single `x = <two-leaf expr>; return x;` alias continuation, an unwritten plain-
        // variable guard value. (The order-independent variant without reassigned-name reads
        // is hoisted before this handler runs; a constant guard value here joins through a
        // temp register whose choice needs the register allocator — defer.)
        if distinct_parameter_reads < 2
            && matches!(value, Expression::Variable(_))
            && matches!(function.return_type, Type::Int | Type::UnsignedInt)
        {
            let [Statement::Assign { name: assigned, value: assigned_value }] = rest else {
                return Ok(false);
            };
            let Some(Expression::Variable(returned)) = function.return_expression.as_ref() else {
                return Ok(false);
            };
            if returned != assigned {
                return Ok(false);
            }
            let two_leaf = matches!(assigned_value, Expression::Binary { left, right, .. }
                if matches!(left.as_ref(), Expression::Variable(_) | Expression::IntegerLiteral(_))
                    && matches!(right.as_ref(), Expression::Variable(_) | Expression::IntegerLiteral(_)));
            if !two_leaf {
                return Ok(false);
            }
            let Expression::Variable(value_name) = value else { return Ok(false) };
            let Some(value_register) = self.lookup_general(value_name) else {
                return Ok(false);
            };
            let result = mwcc_target::Eabi::general_result().number;
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            self.evaluate_tail(assigned_value, function.return_type, result)?;
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
            self.output.instructions.push(Instruction::Or { a: result, s: value_register, b: value_register });
            self.emit_epilogue_and_return();
            return Ok(true);
        }
        Ok(false)
    }

    pub(crate) fn evaluate_body(&mut self, function: &Function) -> Compilation<()> {
        // Drop never-referenced, side-effect-free locals (an unused `int s = 0;`) — mwcc
        // emits nothing for them — then recompile the cleaned function.
        if let Some(cleaned) = remove_dead_locals(function) {
            return self.evaluate_body(&cleaned);
        }
        // A body that CONTINUES past an early-return guard parses the guard into the ordered
        // statement list (`if (c) return v; b = b + 1; return b;` → statements [If, Assign]).
        // When the guard reads only names the rest never writes, guard-first and guard-last
        // emission read the same registers — mwcc compiles both orders identically — so hoist
        // it back into `guards` and let the trailing-guard machinery emit it. A tail that
        // still reads the result register's parameter does NOT fold (mwcc branches in the
        // ordered source but folds through a temp in the flat one — order matters), so it
        // stays ordered for try_ordered_early_return_branch.
        if let Some(hoisted) = self.hoist_order_independent_leading_guards(function) {
            return self.evaluate_body(&hoisted);
        }
        // C89 fdlibm locals (`double z; z = x*x;`) normalize into
        // initializers for the float paths, alternating with the guard
        // hoist through this recursion.
        if let Some(cleaned) = normalize_leading_local_assigns(function) {
            return self.evaluate_body(&cleaned);
        }
        // The TRIG DISPATCHER template claims before the general statement
        // walkers (its leading Assigns would otherwise hit the value-tracking
        // defer).
        if self.try_trig_dispatcher(function)? {
            return Ok(());
        }
        // The ROTATED LOOP likewise (initialized locals route into value
        // tracking otherwise).
        if self.try_rotated_loop(function)? {
            return Ok(());
        }
        if self.try_pipelined_copy(function)? {
            return Ok(());
        }
        if self.try_ctr_loop(function)? {
            return Ok(());
        }
        if self.try_ctr_pair_loop(function)? {
            return Ok(());
        }
        if self.try_norm_loop(function)? {
            return Ok(());
        }
        if self.try_ilogb_diamond(function)? {
            return Ok(());
        }
        if self.try_early_ladder(function)? {
            return Ok(());
        }
        if self.try_indexed_double_return(function)? {
            return Ok(());
        }
        if self.try_punned_pair_ladder(function)? {
            return Ok(());
        }
        if self.try_align_diamond(function)? {
            return Ok(());
        }
        if self.try_writeback_norm(function)? {
            return Ok(());
        }
        // The exact-match whole-function captures (src/captures/).
        if self.try_captures(function)? {
            return Ok(());
        }
        // A body calling a SKIPPED INLINE defers here — after the exact-match
        // templates (a whole-function capture has the inline flattened into
        // its body); the general paths must never emit a bl to the undefined
        // local (wrong bytes — mwcc inlines it).
        if !self.skipped_inline_names.is_empty() && function_calls_any(function, &self.skipped_inline_names) {
            return Err(Diagnostic::error("a call to a skipped inline function needs inline expansion (roadmap)"));
        }
        if self.try_fpclassify_switch(function)? {
            return Ok(());
        }
        // `F t = gf; t();` — a pure fn-pointer alias feeding only the first call's target
        // folds to the direct `gf();` (identical bytes: the pointer loads at the call).
        if let Some(folded) = inline_first_call_target_alias(function) {
            return self.evaluate_body(&folded);
        }
        // Returning a struct BY VALUE (`struct S f(...) { return s; }`) uses the struct-return
        // ABI — a small struct in r3:r4, a larger one via a hidden pointer argument — which is
        // not modeled. Defer rather than emit a bare `blr` that drops the result (a miscompile:
        // the caller would read the input pointer / stale registers as the returned struct).
        if matches!(function.return_type, Type::Struct { .. }) {
            return Err(Diagnostic::error("returning a struct by value is not supported yet (roadmap)"));
        }
        // A store to a global AGGREGATE that addresses through a base register (a struct value's
        // non-offset-0 or large field, or any array element) alongside ANOTHER store: mwcc materializes
        // that base (`li rB,g@sda21` / `lis rB,g@ha`) AHEAD of all the stores; our program-order
        // materialization emits it between the stores, so the bytes differ. Defer when such a
        // base-addressed aggregate store is present and the function has two-plus stores of any kind —
        // a lone store, all-offset-0 small-struct fields (direct SDA21), a pointer's members, and scalar
        // globals (no base register) stay byte-exact.
        {
            let mut total_store_count = 0u32;
            let mut has_base_addressed_aggregate_store = false;
            for statement in &function.statements {
                let Statement::Store { target, .. } = statement else { continue };
                total_store_count += 1;
                match target {
                    // A struct VALUE global's field: offset 0 of a SMALL struct is a direct SDA21 store
                    // (no base register); a non-zero offset or a LARGE (ADDR16) struct needs the base.
                    Expression::Member { base, offset, .. } => {
                        if let Expression::Variable(name) = base.as_ref() {
                            if let Some(Type::Struct { size, .. }) = self.globals.get(name.as_str()) {
                                if *offset != 0 || *size > 8 {
                                    has_base_addressed_aggregate_store = true;
                                }
                            }
                        }
                    }
                    // An array global's element always addresses through a base register (a pointer base
                    // is register-resident already, so it is excluded here).
                    Expression::Index { base, .. } => {
                        if let Expression::Variable(name) = base.as_ref() {
                            if self.global_array_sizes.contains_key(name.as_str()) {
                                has_base_addressed_aggregate_store = true;
                            }
                        }
                    }
                    _ => {}
                }
            }
            if has_base_addressed_aggregate_store && total_store_count >= 2 {
                return Err(Diagnostic::error("a base-addressed global-aggregate store alongside another store needs the shared-base schedule (roadmap)"));
            }
        }
        // `if (gi) f(gi);` — a global read in BOTH an if-condition and its then-body. mwcc loads the
        // global ONCE into the argument register, tests it there, and reuses it for the guarded call
        // (`lwz r3,gi; cmpwi r3,0; beq; bl f`); our codegen loads it into the scratch for the test, then
        // RELOADS it for the body — wrong bytes. Defer until that value is reused across the branch. (A
        // parameter condition, or a body that does not read the condition's global, stays byte-exact.)
        for statement in &function.statements {
            if let Statement::If { condition, then_body, .. } = statement {
                let condition_globals: Vec<&str> = self
                    .globals
                    .keys()
                    .filter(|global| expression_reads_name(condition, global))
                    .map(String::as_str)
                    .collect();
                let body_reads_condition_global = then_body.iter().any(|body_statement| match body_statement {
                    Statement::Expression(expression) => condition_globals.iter().any(|global| expression_reads_name(expression, global)),
                    Statement::Store { value, .. } => condition_globals.iter().any(|global| expression_reads_name(value, global)),
                    _ => false,
                });
                if body_reads_condition_global {
                    return Err(Diagnostic::error("a global read in both an if-condition and its body needs value reuse across the branch (roadmap)"));
                }
            }
        }
        // A long long (64-bit) value lives in a general-register PAIR — r3:r4 is high:low. Route
        // every long-long-involved function to the dedicated handler so none falls through to the
        // 32-bit codegen (which would emit a single-register result for a 64-bit value — wrong
        // bytes). The handler models a narrow set of shapes and defers the rest.
        if matches!(function.return_type, Type::LongLong | Type::UnsignedLongLong)
            || function.parameters.iter().any(|parameter| matches!(parameter.parameter_type, Type::LongLong | Type::UnsignedLongLong))
            || function.locals.iter().any(|local| matches!(local.declared_type, Type::LongLong | Type::UnsignedLongLong))
        {
            return self.emit_long_long(function);
        }
        // `loc = …; return loc` where `loc` is a VARIABLE-INDEXED access (`p[i]`) or a GLOBAL —
        // mwcc reuses the scaled index it already computed (`slwi` once) or the just-stored value,
        // but ours recomputes the index (`slwi` twice) or reloads the global, a byte-different
        // sequence. Defer. (A deref `*p`, a member `s->x`, a const index `p[0]`, and a
        // register param/local are byte-exact and unaffected.)
        if let Some(return_expression) = &function.return_expression {
            for statement in &function.statements {
                if let Statement::Store { target, .. } = statement {
                    if structurally_equal(target, return_expression) {
                        let recomputes_address = matches!(target, Expression::Index { index, .. } if constant_value(index).is_none())
                            || matches!(target, Expression::Variable(name) if self.globals.contains_key(name.as_str()));
                        if recomputes_address {
                            return Err(Diagnostic::error("storing to a variable-indexed or global location then returning it recomputes the address (roadmap)"));
                        }
                    }
                }
            }
        }
        // `global = const; return <const or global>` — mwcc's scheduler computes the return value
        // (a `li` for a constant, an SDA `lwz` for a global) BEFORE the global constant store; ours
        // emits the store first. A param return (already in r3) or a deref/index return is
        // byte-exact and unaffected, as is a non-constant or non-global store.
        if let Some(return_expression) = &function.return_expression {
            let return_is_const_or_global = constant_value(return_expression).is_some()
                || matches!(return_expression, Expression::Variable(name) if self.globals.contains_key(name.as_str()));
            if return_is_const_or_global {
                for statement in &function.statements {
                    if let Statement::Store { target, value } = statement {
                        if constant_value(value).is_some()
                            && matches!(target, Expression::Variable(name) if self.globals.contains_key(name.as_str()))
                        {
                            return Err(Diagnostic::error("a global constant store scheduled around a const/global return is not modeled (roadmap)"));
                        }
                    }
                }
            }
        }
        // A function that takes the address of a variable lowers it to a stack
        // slot (frame-resident); this takes over the whole body. Checked first,
        // since an address-taken variable cannot be value-tracked in a register.
        // The frexp family (locals REASSIGNED across a writeback diamond) runs
        // before the inline pass, which cannot fold reassigned locals.
        // The PUNNED-BITS guard + float-tail composition (the k_sin prefix)
        // claims ahead of the frame families: its x-spill frame form and the
        // float DAG tail are one measured unit.
        if self.try_punned_guard_float_return(function)? {
            return Ok(());
        }
        // The DUAL-TAIL float return (`if (c) return A; else return B;`) —
        // two independent float DAGs behind one compare.
        if self.try_dual_tail_float_return(function)? {
            return Ok(());
        }
        // The conditional-local diamond (`if (c) qx = A; else qx = B;` +
        // float tail) — the k_cos qx form, register variant.
        if self.try_conditional_local_float_return(function)? {
            return Ok(());
        }
        if self.try_frexp_family(function)? {
            return Ok(());
        }
        // THE COMPOSER: the full three-arm s_floor ladder.
        if self.try_punned_ladder_writeback(function)? {
            return Ok(());
        }
        // THE MODF LADDER: pointer stores + integral/fraction returns.
        if self.try_punned_modf_ladder(function)? {
            return Ok(());
        }
        // The SHIFT-WRITEBACK family (s_floor arm2's core) parses the
        // un-normalized leading assigns itself — its mutations reassign
        // punned locals, which the initializer normalizer refuses.
        if self.try_punned_shift_writeback(function)? {
            return Ok(());
        }
        // The punned-guard WRITEBACK (the s_floor tail) binds its punned
        // locals to scratch registers — ahead of the inline-away pass that
        // would dissolve them into repeated frame reads.
        if self.try_punned_guard_writeback(function)? {
            return Ok(());
        }
        // The raise family (a fn-pointer local live across calls) likewise.
        if self.try_raise_family(function)? {
            return Ok(());
        }
        // Register locals feeding a frame-resident body (`int hx = *(int*)&x; return
        // f(hx);`) inline away first: the frame path cannot bind them, and once
        // substituted the body is the proven direct form (`return f(*(int*)&x);`).
        if let Some(inlined) = inline_frame_feeding_locals(function) {
            return self.evaluate_body(&inlined);
        }
        if self.try_frame_resident(function)? {
            return Ok(());
        }
        // A counting `for (i = 0; i < bound; i++)` loop owns its single local
        // counter, so it is checked before the value-tracking path claims it.
        if self.try_for_counter(function)? {
            return Ok(());
        }
        // A leaf non-counting `while`/`do-while` whose body is pure in-place increments
        // (`while (*p) p++;`) lowers to the rotated form; claimed before value-tracking since the
        // loop-carried increment must emit in place.
        if self.try_emit_increment_while(function)? {
            return Ok(());
        }
        // `T y; if (c) y = A; else y = B; return y;` — both arms assign the returned
        // local, so the whole body is the select `return (c) ? A : B`.
        if self.try_conditional_assign(function)? {
            return Ok(());
        }
        // `T y = INIT; if (c) y = NEW; return y;` (no else) where INIT is a variable ALREADY
        // resident in the result register (the common param-0 case): the clean in-place branch
        // form `<test c>; b<!c>lr; <NEW into result>; blr` (min/max/abs/clamp). NEW may be any
        // evaluable expression (neg/mr/li/add/…), unlike the leaf-only initialized handler below.
        if self.try_conditional_overwrite_inplace(function)? {
            return Ok(());
        }
        // `T y = INIT; if (c) y = NEW; return y;` (no else), constant arms — mwcc lowers the
        // conditional ASSIGN as an early-return branch form (NOT the select/branchless idiom).
        if self.try_conditional_assign_initialized(function)? {
            return Ok(());
        }
        // `if (c) { [g = w;] [v = NEW;] } return v;` over a PARAMETER — the in-place
        // diamond with the merge `mr r3,v`, folding to a conditional return when v is r3.
        if self.try_guard_block_mutations(function)? {
            return Ok(());
        }
        if self.try_conditional_reassign_return(function)? {
            return Ok(());
        }
        // A function's value-tracked locals are folded into its stores and trailing return,
        // then recompiled — `int x = a; gi = x; x = b; gj = x;` becomes `gi = a; gj = b;`,
        // and `int x = a; gi = x; return x;` becomes `gi = a; return a;`. The store paths
        // (or the un-schedulable-store deferral) own the cleaned body. Checked before the
        // value-tracking path, which cannot fold a void function's store-feeding locals.
        if let Some(inlined) = inline_store_bearing_locals(function) {
            return self.evaluate_body(&inlined);
        }
        // `int x = foo(...); gi = x;` / `int x = foo(...); return x;` — a single-use call
        // result stored directly or returned. The result lives in r3 and is not live across
        // another call, so both are byte-exact; inline the local. A second call or use
        // defers to the callee-saved allocator.
        if let Some(inlined) = inline_single_call_result(function) {
            return self.evaluate_body(&inlined);
        }
        // Value-tracked locals (reassignment, multiple locals) are inlined into the
        // return expression and compiled there; this takes over the whole body when
        // it applies, leaving the straight-line paths below byte-identical.
        // A single ordered early-return guard over a value-tracked continuation, where the
        // constant fold does not apply — the real forward-branch form.
        if self.try_ordered_early_return_branch(function)? {
            return Ok(());
        }
        // The FLOAT DAG arm claims double multiply-add trees with named
        // double locals BEFORE value tracking and the int-oriented folds:
        // folding a single-use float local (v = z*x) duplicates the shared z
        // subterm, while mwcc keeps locals as window-top-tier shared
        // registers.
        if self.try_float_dag_return(function)? {
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            return Ok(());
        }
        if self.try_float_param_reassign(function)? {
            return Ok(());
        }
        if self.try_live_across_branches(function)? {
            return Ok(());
        }
        if self.try_value_tracking(function)? {
            return Ok(());
        }
        // Fold single-assignment, return-only locals (no call in their initializers)
        // into the return, then recompile — `int z = x + 1; g(); return z;` becomes the
        // equivalent `g(); return x + 1;`, which the parameter-preservation path emits.
        if let Some(inlined) = inline_return_only_locals(function) {
            return self.evaluate_body(&inlined);
        }
        // A value-tracked local feeding a single switch's scrutinee/arms inlines into the switch and
        // recompiles, so `int m = n + 1; switch(m)` lowers like the direct `switch(n + 1)`.
        if let Some(inlined) = inline_switch_scrutinee_locals(function) {
            return self.evaluate_body(&inlined);
        }
        // A leaf void body that is purely constant stores of one repeated value
        // (struct/array zeroing) materializes the value once and reuses it.
        if self.try_constant_store_fill(function)? {
            return Ok(());
        }
        // A whole-body `if (c) { <constant run> } else { <constant run> }`: branch over the then-arm
        // to the else, each arm the batched constant store run then its own `blr`.
        if self.try_constant_store_if_else(function)? {
            return Ok(());
        }
        // Two computed-value stores to distinct SDA globals: mwcc overlaps the two value
        // computations (both into registers, then both stores), which the sequential path
        // does not. The allocator places the first value off the scratch (live across the
        // second), the second into r0.
        if self.try_computed_store_fill(function)? {
            return Ok(());
        }
        // The same overlap with one computed value and one register-leaf value (`gi=a+1;
        // gj=b;`): the leaf is stored first (ready), the computed second.
        if self.try_mixed_store_fill(function)? {
            return Ok(());
        }
        // Three+ stores of register leaves with a single constant interspersed (`gi=a;
        // gj=b; gk=5;`): the constant's `li` is hoisted and the stores keep source order
        // (a leading constant swaps off the latency slot).
        if self.try_leaf_constant_fill(function)? {
            return Ok(());
        }
        // Leaf multi-store bodies of COMPUTED int values through the measured
        // models — the DAG emitter (linearize + assign_registers). Runs after
        // the proven store-fill arms, catching what they defer.
        if self.try_dag_store_fill(function)? {
            return Ok(());
        }
        // Multiple stores where a value loads a float/double global reschedule the loads
        // (mwcc loads the global once and reuses it across the stores); not modeled, so
        // DEFER rather than emit a redundant load per store. A single such store (`gf =
        // gg;`) needs no scheduling and stays byte-exact.
        if function.guards.is_empty()
            && function.locals.is_empty()
            && !function_makes_call(function)
            && function.statements.len() >= 2
        {
            let loads_float_global = |generator: &Self, value: &Expression| {
                matches!(value, Expression::Variable(name)
                    if !generator.locations.contains_key(name.as_str())
                        && matches!(generator.globals.get(name.as_str()), Some(Type::Float | Type::Double)))
            };
            let all_stores = function.statements.iter().all(|statement| matches!(statement, Statement::Store { .. }));
            let any_float_global = function
                .statements
                .iter()
                .any(|statement| matches!(statement, Statement::Store { value, .. } if loads_float_global(self, value)));
            if all_stores && any_float_global {
                return Err(Diagnostic::error("multiple stores loading a float global need the load scheduler (roadmap)"));
            }
        }
        // Un-schedulable multi-store: a body whose statements are 2+ stores to SDA integer
        // globals that the fills above did not absorb (a trailing return, if any, is
        // separate). mwcc latency-schedules these (load/computation hoisting, constant-`li`
        // slot fill); the normal sequential emission would not reproduce that, so DEFER
        // rather than ship wrong bytes. Only an all-distinct-leaf run (no computation to
        // schedule, no dead store) stays byte-exact on the normal path, so let that through.
        if function.guards.is_empty()
            && function.locals.is_empty()
            && !function_makes_call(function)
            && function.statements.len() >= 2
            && self.behavior.global_addressing == GlobalAddressing::SmallData
        {
            let mut targets = Vec::new();
            let mut all_leaves = true;
            let mut all_sda_integer_stores = true;
            for statement in &function.statements {
                let Statement::Store { target: Expression::Variable(name), value } = statement else {
                    all_sda_integer_stores = false;
                    break;
                };
                match self.globals.get(name.as_str()) {
                    Some(global_type) if !matches!(global_type, Type::Float | Type::Double) => targets.push(name.as_str()),
                    _ => {
                        all_sda_integer_stores = false;
                        break;
                    }
                }
                if !matches!(value, Expression::Variable(leaf) if !self.globals.contains_key(leaf.as_str())) {
                    all_leaves = false;
                }
            }
            if all_sda_integer_stores {
                let distinct = {
                    let mut sorted = targets.clone();
                    sorted.sort_unstable();
                    sorted.dedup();
                    sorted.len() == targets.len()
                };
                if !all_leaves || !distinct {
                    return Err(Diagnostic::error("a run of stores that mwcc latency-schedules needs the scheduler (roadmap)"));
                }
            }
        }
        // A single COMPUTED store to an SDA integer global plus an int return that
        // does NOT read the stored global: mwcc's DAG scheduler interleaves the
        // return-value computation with the store chain; sequential emission
        // diverges. try_dag_store_fill (above) claims every such shape it has
        // vocabulary for, so what reaches here (a division, an unsigned shift)
        // would fall through to the sequential emitter — defer. A return that
        // reads the just-stored global (rand.c) is data-dependent, so mwcc is
        // sequential too — byte-exact on the normal path; let it through.
        if function.guards.is_empty()
            && function.locals.is_empty()
            && !function_makes_call(function)
            && self.behavior.global_addressing == GlobalAddressing::SmallData
            && !matches!(function.return_type, Type::Void | Type::Float | Type::Double)
        {
            if let (Some(return_expression), [Statement::Store { target: Expression::Variable(name), value }]) =
                (&function.return_expression, function.statements.as_slice())
            {
                let sda_integer_global = matches!(self.globals.get(name.as_str()), Some(global_type) if !matches!(global_type, Type::Float | Type::Double));
                let leaf_value = constant_value(value).is_some()
                    || matches!(value, Expression::Variable(leaf) if !self.globals.contains_key(leaf.as_str()));
                if sda_integer_global && !leaf_value && count_name_occurrences(return_expression, name) == 0 {
                    return Err(Diagnostic::error("a computed store scheduled against an independent return needs the DAG scheduler (roadmap)"));
                }
            }
        }
        // A `do { …calls… } while (--counter);` loop: the counter goes in r31
        // (callee-saved), the body branches back, and the decrement-and-test is a
        // single `addic.`/`bne`.
        if self.try_do_while_counter(function)? {
            return Ok(());
        }
        // A function whose body is a single `switch` lowers to the dispatch tree:
        // the comparisons, then the case bodies, then the default (the `default:`
        // arm if present, else the function's trailing `return`). The cases and
        // default each end in their own `blr`, so this owns the whole body.
        if let [Statement::Switch { scrutinee, arms, default }] = function.statements.as_slice() {
            if function.guards.is_empty() && function.locals.is_empty() && !function_makes_call(function) {
                let default_expression = default
                    .as_ref()
                    .or(function.return_expression.as_ref())
                    .ok_or_else(|| Diagnostic::error("a switch with no default needs a trailing return"))?;
                let result = match function.return_type {
                    Type::Float | Type::Double => return Err(Diagnostic::error("a floating-point switch result is not supported yet (roadmap)")),
                    Type::Void => return Err(Diagnostic::error("a void switch is not supported yet (roadmap)")),
                    _ => Eabi::general_result().number,
                };
                return self.emit_switch(scrutinee, arms, default_expression, default.is_some(), function.return_type, result);
            }
        }
        // A non-leaf function whose whole body is `if (c) <call>;`: mwcc schedules
        // the condition test (`cmpwi`) into the prologue, between `mflr` and the LR
        // store, then branches forward over the body to the epilogue when false.
        if let [Statement::If { condition, then_body, else_body }] = function.statements.as_slice() {
            if function_makes_call(function)
                && function.return_type == Type::Void
                && function.guards.is_empty()
                && else_body.is_empty()
                && !then_body.is_empty()
                // A straight-line body (calls/stores, no nested control flow); a value
                // read across one of its calls would need callee-saving, so defer it.
                && then_body.iter().all(|statement| matches!(statement, Statement::Store { .. } | Statement::Expression(_) | Statement::Assign { .. }))
                && !reads_value_across_call(function)
            {
                self.non_leaf = true;
                self.frame_size = 16;
                // The if's join label advances mwcc's anonymous-`@N` counter by 2.
                self.output.anonymous_label_bump = 2;
                self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
                self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
                let condition_start = self.output.instructions.len();
                let (options, condition_bit) = self.emit_condition_test(condition)?;
                // mwcc fills the mflr->LR-store latency slot with the condition test only
                // when it is a bare compare (a register operand). A member/complex
                // condition loads into r0, which would clobber the just-saved LR, so the
                // LR store must come first — otherwise it would save the loaded value, not
                // the return address.
                // mwcc fills the mflr->LR-store latency slot with the FIRST condition
                // instruction when it does not write r0 (a compare, or a float load/
                // compare targeting cr/FP), issuing the LR store right after it (e.g.
                // `lfs f0; stw r0,20; fcmpo`). An integer load / rlwinm. / extsb. into r0
                // would clobber the saved LR, so the store precedes the whole condition.
                let first_writes_r0 = self.output.instructions.get(condition_start).map_or(false, |instruction| {
                    match instruction {
                        // Compares and float/cr ops write cr0/an FPR, not a GPR.
                        Instruction::CompareWord { .. }
                        | Instruction::CompareWordImmediate { .. }
                        | Instruction::CompareLogicalWord { .. }
                        | Instruction::CompareLogicalWordImmediate { .. }
                        | Instruction::FloatCompareOrdered { .. }
                        | Instruction::FloatCompareUnordered { .. }
                        | Instruction::LoadFloatSingle { .. }
                        | Instruction::LoadFloatSingleIndexed { .. }
                        | Instruction::LoadFloatDouble { .. }
                        | Instruction::LoadFloatDoubleIndexed { .. }
                        | Instruction::ConditionRegisterOr { .. } => false,
                        // A narrow extension into a non-r0 GPR — `extsh r3,r3`, the first
                        // operand of a two-operand narrow compare — leaves the saved LR in r0
                        // intact, so the store still fills the slot after it. Extending into
                        // r0 (a narrow leaf against a constant) clobbers it: store first.
                        Instruction::ExtendSignByte { a, .. }
                        | Instruction::ExtendSignByteRecord { a, .. }
                        | Instruction::ExtendSignHalfword { a, .. }
                        | Instruction::ExtendSignHalfwordRecord { a, .. }
                        | Instruction::ClearLeftImmediate { a, .. }
                        | Instruction::ClearLeftImmediateRecord { a, .. } => *a == 0,
                        // Any other first instruction writes a GPR (a load into r0, rlwinm.).
                        _ => true,
                    }
                });
                let lr_position = if first_writes_r0 { condition_start } else { condition_start + 1 };
                self.output.instructions.insert(lr_position, Instruction::StoreWord { s: 0, a: 1, offset: 20 });
                // The insert shifts the condition instructions at/after it down by one, so
                // their relocations (a global condition's SDA21 reloc) must shift too.
                for relocation in &mut self.output.relocations {
                    if relocation.instruction_index >= lr_position {
                        relocation.instruction_index += 1;
                    }
                }
                let branch_index = self.output.instructions.len();
                self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
                for statement in then_body {
                    self.emit_statement(statement)?;
                }
                let label = self.output.instructions.len();
                if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                    *target = label;
                }
                self.emit_epilogue_and_return();
                return Ok(());
            }
        }
        // A non-leaf `if (c) { then } else { else }` with straight-line bodies: the
        // condition test schedules into the prologue, `beq` jumps to the else body,
        // the then body falls through to an unconditional `b` over the else body to
        // the shared epilogue.
        if let [Statement::If { condition, then_body, else_body }] = function.statements.as_slice() {
            if function_makes_call(function)
                && function.return_type == Type::Void
                && function.guards.is_empty()
                && !then_body.is_empty()
                && !else_body.is_empty()
                && then_body.iter().chain(else_body).all(|statement| matches!(statement, Statement::Store { .. } | Statement::Expression(_) | Statement::Assign { .. }))
                && !reads_value_across_call(function)
            {
                self.non_leaf = true;
                self.frame_size = 16;
                // The else branch and join label advance mwcc's anonymous-`@N` counter.
                self.output.anonymous_label_bump = 3;
                self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
                self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
                let condition_start = self.output.instructions.len();
                let (options, condition_bit) = self.emit_condition_test(condition)?;
                // mwcc fills the mflr->LR-store latency slot with the condition test only
                // when it is a bare compare (a register operand). A member/complex
                // condition loads into r0, which would clobber the just-saved LR, so the
                // LR store must come first — otherwise it would save the loaded value, not
                // the return address.
                // mwcc fills the mflr->LR-store latency slot with the FIRST condition
                // instruction when it does not write r0 (a compare, or a float load/
                // compare targeting cr/FP), issuing the LR store right after it (e.g.
                // `lfs f0; stw r0,20; fcmpo`). An integer load / rlwinm. / extsb. into r0
                // would clobber the saved LR, so the store precedes the whole condition.
                let first_writes_r0 = self.output.instructions.get(condition_start).map_or(false, |instruction| {
                    match instruction {
                        // Compares and float/cr ops write cr0/an FPR, not a GPR.
                        Instruction::CompareWord { .. }
                        | Instruction::CompareWordImmediate { .. }
                        | Instruction::CompareLogicalWord { .. }
                        | Instruction::CompareLogicalWordImmediate { .. }
                        | Instruction::FloatCompareOrdered { .. }
                        | Instruction::FloatCompareUnordered { .. }
                        | Instruction::LoadFloatSingle { .. }
                        | Instruction::LoadFloatSingleIndexed { .. }
                        | Instruction::LoadFloatDouble { .. }
                        | Instruction::LoadFloatDoubleIndexed { .. }
                        | Instruction::ConditionRegisterOr { .. } => false,
                        // A narrow extension into a non-r0 GPR — `extsh r3,r3`, the first
                        // operand of a two-operand narrow compare — leaves the saved LR in r0
                        // intact, so the store still fills the slot after it. Extending into
                        // r0 (a narrow leaf against a constant) clobbers it: store first.
                        Instruction::ExtendSignByte { a, .. }
                        | Instruction::ExtendSignByteRecord { a, .. }
                        | Instruction::ExtendSignHalfword { a, .. }
                        | Instruction::ExtendSignHalfwordRecord { a, .. }
                        | Instruction::ClearLeftImmediate { a, .. }
                        | Instruction::ClearLeftImmediateRecord { a, .. } => *a == 0,
                        // Any other first instruction writes a GPR (a load into r0, rlwinm.).
                        _ => true,
                    }
                });
                let lr_position = if first_writes_r0 { condition_start } else { condition_start + 1 };
                self.output.instructions.insert(lr_position, Instruction::StoreWord { s: 0, a: 1, offset: 20 });
                // The insert shifts the condition instructions at/after it down by one, so
                // their relocations (a global condition's SDA21 reloc) must shift too.
                for relocation in &mut self.output.relocations {
                    if relocation.instruction_index >= lr_position {
                        relocation.instruction_index += 1;
                    }
                }
                let branch_to_else = self.output.instructions.len();
                self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
                for statement in then_body {
                    self.emit_statement(statement)?;
                }
                let branch_to_join = self.output.instructions.len();
                self.output.instructions.push(Instruction::Branch { target: 0 });
                let else_label = self.output.instructions.len();
                if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_to_else] {
                    *target = else_label;
                }
                for statement in else_body {
                    self.emit_statement(statement)?;
                }
                let join_label = self.output.instructions.len();
                if let Instruction::Branch { target } = &mut self.output.instructions[branch_to_join] {
                    *target = join_label;
                }
                self.emit_epilogue_and_return();
                return Ok(());
            }
        }
        // A non-leaf function led by `if (c) { …calls…; return X; }` with a
        // continuation that supplies the other exit: mwcc schedules the condition
        // test into the prologue, the early return materializes X and branches to a
        // SHARED epilogue, and the continuation falls into that same epilogue.
        if self.try_non_leaf_if_first_early_return(function)? {
            return Ok(());
        }
        // A function that calls is non-leaf: save the link register around a 16-byte
        // frame before doing anything else.
        let mut lr_store_index: Option<usize> = None;
        if function_makes_call(function) {
            if !function.guards.is_empty() {
                return Err(Diagnostic::error("calls combined with guards not yet supported"));
            }
            // `*a = g(); *b = h();` — 2–4 output pointers saved in r31/r30/… across their calls.
            // Runs before the general callee-saved path, which would otherwise emit the stores
            // through the raw (clobbered) argument registers and defer/miscompile.
            if self.try_stores_through_pointers(function)? {
                return Ok(());
            }
            // `int t = gi; g(); return t;` — a memory-loaded local carried across calls in r31.
            if self.try_callee_saved_memory_local(function)? {
                return Ok(());
            }
            // `F t = gf; if (!t) return; t();` — a guarded call through a global fn-pointer.
            if self.try_guarded_global_pointer_call(function)? {
                return Ok(());
            }
            // Parameters live across the call go in callee-saved registers (r31
            // descending), saved in the prologue and reloaded in the epilogue.
            if self.try_frsqrte_sqrt(function)? {
                return Ok(());
            }
            if self.try_float_callee_saved(function)? {
                return Ok(());
            }
            if self.try_callee_saved(function)? {
                return Ok(());
            }
            if self.try_callee_saved_call_result(function)? {
                return Ok(());
            }
            // `*p = g();` — a call's result stored through a pointer parameter saved in r31.
            if self.try_store_call_through_pointer(function)? {
                return Ok(());
            }
            if self.try_callee_saved_computed_local(function)? {
                return Ok(());
            }
            // A parameter passed to several calls in turn (`g(x); h(x);`) — saved in r31,
            // the first call uses the incoming register, later calls restore from r31.
            if self.try_callee_saved_call_args(function)? {
                return Ok(());
            }
            // `return f(...) + x;` — a live parameter combined with a call's result in the return.
            if self.try_callee_saved_call_combine(function)? {
                return Ok(());
            }
            // `p(x); q(y);` — two params passed to two calls in turn; the later param is preserved.
            if self.try_callee_saved_call_sequence(function)? {
                return Ok(());
            }
            // `g(x); return x OP y;` — two params both live across one call, combined in the return.
            if self.try_callee_saved_param_pair_combine(function)? {
                return Ok(());
            }
            // `return f() OP g();` — two call results combined in the return.
            if self.try_callee_saved_two_call_combine(function)? {
                return Ok(());
            }
            // Byte-exact-or-defer: a value (parameter or register local) read after a
            // call is read from a register the call clobbered. mwcc preserves it in a
            // callee-saved register (r31…) — multi-value/local cases are the next
            // step; until then DEFER rather than emit a read of the clobbered register.
            if reads_value_across_call(function) {
                return Err(Diagnostic::error("a value live across a call needs the callee-saved register allocator (roadmap)"));
            }
            self.non_leaf = true;
            self.frame_size = 16;
            self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
            self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
            self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
            lr_store_index = Some(self.output.instructions.len() - 1);
        }

        // A leading store (or store run) before a trailing `if` needs mwcc's cross-statement
        // scheduler: it hoists the if's condition test as early as possible — into the leading
        // store's value-materialize latency gap (`li r0,1; cmpwi; stw r0,g; beqlr; …`) or to the
        // front. The sequential emission below instead emits the store fully, then the test — a
        // DIFFERS — so defer this shape. (A whole-body store run, or a whole-body trailing `if`,
        // are handled byte-exactly by the store-fill matchers above.)
        if let [leading @ .., Statement::If { .. }] = function.statements.as_slice() {
            if !leading.is_empty() && leading.iter().all(|statement| matches!(statement, Statement::Store { .. })) {
                return Err(Diagnostic::error("a leading store before a trailing if needs the cross-statement scheduler (roadmap)"));
            }
        }

        // A leading early-return if whose continuation MATERIALIZES store values (a
        // constant/computed value, or several stores) schedules the return value between
        // the materialization and the store (`li r0,5; li r3,0; stw r0`), or interleaves
        // a store batch — the sequential emission below would emit the store first, a
        // byte-DIFF. The verified single-constant-store form is handled by
        // try_ordered_early_return_branch; everything else here defers. (A store of a
        // plain register value needs no materialization and stays — verified.)
        if let [Statement::If { then_body, .. }, continuation @ ..] = function.statements.as_slice() {
            if matches!(then_body.as_slice(), [Statement::Return(_)]) {
                let store_count = continuation
                    .iter()
                    .filter(|statement| matches!(statement, Statement::Store { .. }))
                    .count();
                let materializing_store = continuation.iter().any(|statement| {
                    matches!(statement, Statement::Store { value, .. }
                        if !matches!(value, Expression::Variable(name) if self.locations.contains_key(name.as_str())))
                });
                // A computed-index GLOBAL-ARRAY target materializes its ADDRESS
                // (lis/slwi/addi) even for a register value — with a live return, mwcc
                // keeps the base out of the index register and interleaves the return
                // (`addi r5,r5; li r3,0; stwx r4,r5,r0`), which the sequential emission
                // below does not model. (A pointer-parameter target needs no address
                // build and stays — verified.)
                let address_materializing_store = continuation.iter().any(|statement| {
                    matches!(statement, Statement::Store { target: Expression::Index { base, index }, .. }
                        if matches!(base.as_ref(), Expression::Variable(name) if self.globals.contains_key(name.as_str()))
                            && constant_value(index).is_none())
                });
                if store_count >= 2 || materializing_store || address_materializing_store {
                    return Err(Diagnostic::error(
                        "an early-return continuation that materializes store values needs the store/return scheduler (roadmap)",
                    ));
                }
            }
        }

        // Body statements (stores, calls) run first.
        let statement_count = function.statements.len();
        for (index, statement) in function.statements.iter().enumerate() {
            // A trailing `if (c) { body }` in a leaf void function: the false path
            // is the function exit, so it is a conditional return, then the body,
            // then the normal `blr`. (Non-leaf needs a forward branch to the
            // epilogue, and a non-final if needs to skip forward — both deferred.)
            if let Statement::If { condition, then_body, else_body } = statement {
                // A leaf if whose then-body is at most one statement then an early
                // `return`, with a continuation after it (more statements or the
                // trailing return): forward-branch over the body, the return is an
                // exit, and the branch lands on the continuation. Two or more
                // leading statements (constant stores mwcc would interleave) need
                // the scheduler. With no continuation (a trailing void if) the
                // false path is the immediate exit, which is a `beqlr` form — that
                // and the multi-statement case defer.
                let has_continuation = index + 1 < statement_count || function.return_expression.is_some();
                // A trailing void `if (c) { stmt; return; }` (nothing after): the
                // `return;` coincides with the function exit, so drop it and use
                // the conditional-return (`beqlr`) form of a plain trailing if.
                if !function_makes_call(function)
                    && else_body.is_empty()
                    && !has_continuation
                    && function.return_type == Type::Void
                    && then_body.len() == 2
                    && matches!(then_body.last(), Some(Statement::Return(None)))
                {
                    self.emit_trailing_if(condition, &then_body[..1], else_body)?;
                    continue;
                }
                // A trailing-void, no-else if-BLOCK of two-plus REGISTER-VALUED stores (each value
                // already in a register — nothing to materialize or schedule): the conditional
                // return then the stores in source order. A constant/global/computed value needs the
                // batch scheduler, so emit_trailing_if defers those.
                if !function_makes_call(function)
                    && else_body.is_empty()
                    && index + 1 == statement_count
                    && function.return_type == Type::Void
                    && then_body.len() >= 2
                    && then_body.iter().all(|inner| matches!(inner,
                        Statement::Store { value: Expression::Variable(name), .. } if self.locations.contains_key(name.as_str())))
                {
                    self.emit_trailing_if(condition, then_body, else_body)?;
                    continue;
                }
                if !function_makes_call(function)
                    && else_body.is_empty()
                    && then_body.len() <= 2
                    && has_continuation
                    && matches!(then_body.last(), Some(Statement::Return(_)))
                    // A store before a VALUE return must be INTERLEAVED with the return-value
                    // computation the way mwcc's scheduler does (`li r0,V; li r3,R; stw r0`, not
                    // `li r0,V; stw r0; li r3,R`) — that needs the keystone scheduler (#20), so
                    // defer it. A valueless `return;` has no value to interleave (store + bare
                    // epilogue is byte-exact), and a value-tracked Assign emits nothing here, so
                    // both of those stay byte-exact.
                    && (matches!(then_body.last(), Some(Statement::Return(None)))
                        || then_body[..then_body.len() - 1].iter().all(|statement| matches!(statement, Statement::Assign { .. })))
                {
                    self.emit_if_early_return(condition, then_body, function.return_type)?;
                    continue;
                }
                // Single-statement leaf if-blocks. A multi-statement body needs the
                // instruction scheduler, and a non-leaf if needs the cmpwi scheduled
                // into the prologue — both defer for now.
                if then_body.len() == 1 && !function_makes_call(function) {
                    let trailing_void = index + 1 == statement_count && function.return_type == Type::Void;
                    if trailing_void {
                        // The false path is the function exit (or the else / else-if):
                        // a conditional return, or a branch into the else chain.
                        self.emit_trailing_if(condition, then_body, else_body)?;
                        continue;
                    }
                    if else_body.is_empty() {
                        // A conditional store to a global that the very NEXT statement
                        // unconditionally overwrites is a DEAD store: mwcc drops the whole `if`
                        // (the condition has no side effect here — this branch is call-free) and
                        // emits only the final store. We do not do that dead-store elimination, so
                        // emitting both stores faithfully would diverge — defer instead.
                        fn store_target(statement: &Statement) -> Option<&str> {
                            match statement {
                                Statement::Store { target: Expression::Variable(name), .. } => Some(name.as_str()),
                                _ => None,
                            }
                        }
                        if let Some(dead) = store_target(&then_body[0]) {
                            if function.statements.get(index + 1).and_then(store_target) == Some(dead) {
                                return Err(Diagnostic::error("a dead conditional store (overwritten by the next statement) needs dead-store elimination (roadmap)"));
                            }
                        }
                        // The false path skips the body: forward branch.
                        self.emit_if_forward(condition, then_body)?;
                        continue;
                    }
                }
                // A non-trailing multi-store if-BLOCK that is the FIRST statement of a void body and
                // is followed by exactly one trailing store: `cmpwi; beq cont; <then run>; cont:
                // <trailing store>; blr`. The if-first restriction avoids the leading-store-before-if
                // scheduler; the single trailing store is what the loop emits byte-exactly next. A
                // register-valued then-run stores sequentially, a constant one materializes batched.
                if !function_makes_call(function)
                    && else_body.is_empty()
                    && function.return_type == Type::Void
                    && index == 0
                    && statement_count == 2
                    && matches!(function.statements.get(1), Some(Statement::Store { .. }))
                    && then_body.len() >= 2
                {
                    let then_plan = self.constant_store_run_plan(then_body);
                    if then_plan.is_some() || self.store_run_arm_registers(then_body) {
                        let (options, condition_bit) = self.emit_condition_test(condition)?;
                        let branch_index = self.output.instructions.len();
                        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
                        match then_plan {
                            Some(plan) => self.emit_constant_store_run(then_body, plan)?,
                            None => for statement in then_body { self.emit_statement(statement)?; },
                        }
                        let label = self.output.instructions.len();
                        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                            *target = label;
                        }
                        continue;
                    }
                }
            }
            self.emit_statement(statement)?;
        }

        // Hoist a leading register move from the body's statements (a call's argument
        // setup) into the prologue's mflr->LR-store slot.
        self.hoist_leading_arg_moves(lr_store_index);

        // A `void` function ends after its statements.
        if function.return_type == Type::Void {
            self.emit_epilogue_and_return();
            return Ok(());
        }

        let result = match function.return_type {
            Type::Float | Type::Double => Eabi::float_result().number,
            _ => Eabi::general_result().number,
        };
        let return_expression = function
            .return_expression
            .as_ref()
            .ok_or_else(|| Diagnostic::error("a non-void function needs a return value"))?;

        if !function.guards.is_empty() {
            // Guard + single value-tracked local, zero-select: `int x = a+1; if (c) return 0;
            // return x;` (or `if (c) return x; return 0;`). mwcc materializes the local in
            // the result register but SCHEDULES the materialization into the select's
            // neg->or latency slot — `neg r0,c; addi r3,a,1; or r0,r0,c; srawi r0,31; and/
            // andc r3,r3,r0` (the addi AFTER the leading neg). Emit that interleave directly:
            // leading neg, the local, then the mask combine. Restricted to a single-op
            // integer local, a leaf condition, no statements, and exactly one arm the
            // constant 0 (the other the local).
            if let ([local], [guard]) = (function.locals.as_slice(), function.guards.as_slice()) {
                let zero_is_then = matches!(guard.value, Expression::IntegerLiteral(0));
                let zero_is_else = matches!(return_expression, Expression::IntegerLiteral(0));
                let local_is_other = (zero_is_then && matches!(return_expression, Expression::Variable(name) if *name == local.name))
                    || (zero_is_else && matches!(&guard.value, Expression::Variable(name) if *name == local.name));
                let condition_register = leaf_name(&guard.condition).and_then(|name| self.lookup_general(name));
                let initializer = local.initializer.as_ref();
                if local_is_other
                    && function.statements.is_empty()
                    && initializer.is_some_and(|init| self.is_single_op_register_value(init))
                    && class_of(local.declared_type)? == ValueClass::General
                {
                    if let (Some(condition_register), Some(initializer)) = (condition_register, initializer) {
                        self.output.instructions.push(Instruction::Negate { d: GENERAL_SCRATCH, a: condition_register });
                        self.evaluate(initializer, local.declared_type, result)?;
                        self.output.instructions.push(Instruction::Or { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, b: condition_register });
                        self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: GENERAL_SCRATCH, s: GENERAL_SCRATCH, shift: 31 });
                        self.output.instructions.push(if zero_is_then {
                            Instruction::AndComplement { a: result, s: result, b: GENERAL_SCRATCH }
                        } else {
                            Instruction::And { a: result, s: result, b: GENERAL_SCRATCH }
                        });
                        self.output.instructions.push(Instruction::BranchToLinkRegister);
                        return Ok(());
                    }
                }
            }
            if !function.locals.is_empty() {
                return Err(Diagnostic::error("locals combined with guards not yet supported"));
            }
            // mwcc lowers a single guard as a select (working-register form) but a
            // chain of guards as separate return blocks.
            if let [guard] = function.guards.as_slice() {
                // A logical (&&/||) condition short-circuits straight into the two return
                // blocks rather than computing the operator as a 0/1 value.
                if self.try_emit_short_circuit_guard(&guard.condition, &guard.value, return_expression, result)? {
                    return Ok(());
                }
                // `if (c) return X; return X` is degenerate: both paths return the same
                // value, and mwcc keeps the dead condition test then a single `blr`. Defer
                // rather than emit a spurious conditional return for the matching arms.
                if let (Expression::Variable(value_name), Expression::Variable(return_name)) = (&guard.value, return_expression) {
                    if value_name == return_name {
                        return Err(Diagnostic::error("a guard whose value equals the fall-through return is degenerate (roadmap)"));
                    }
                }
                // A guard condition that is a FLOAT comparison against a float CONSTANT (`if (a > 0.0f)
                // return 1; return 0;`) folds to the branchless `(a OP k) ? v : w` — the .text is
                // byte-exact — but mwcc allocates the if's (folded-away) branch labels BEFORE the pooled
                // float constant, so the constant's anonymous `@N` symbol number is offset by 2 from
                // ours. Modeling that counter is the low-value @N seam, so defer rather than emit a
                // mismatched `@N` symbol. (A non-guard `return a > 0.0f;` has no phantom labels and
                // matches; a two-variable float compare `a < b` pools no constant and is unaffected.)
                if matches!(&guard.condition, Expression::Binary { operator, left, right }
                    if crate::analysis::is_comparison(*operator)
                        && (matches!(left.as_ref(), Expression::FloatLiteral(_)) || matches!(right.as_ref(), Expression::FloatLiteral(_))))
                {
                    return Err(Diagnostic::error("a float-constant guard condition's pooled @N symbol is offset by mwcc's folded branch labels (roadmap)"));
                }
                // A null-guarded dereference (`if (!p) return CONST; return *p;` or the mirror
                // `if (p) return *p; return CONST;`) cannot fold branchless — dereferencing null is
                // unsafe — so mwcc branches on `p == 0` to the cold constant with the access in the
                // fall-through: `cmplwi p,0; beq COLD; <hot access>; blr; COLD: li CONST; blr`.
                if let Some((pointer, hot, cold)) = guarded_null_dereference(&guard.condition, &guard.value, return_expression, function.return_type) {
                    if let Some(pointer_register) = self.lookup_general(pointer) {
                        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: pointer_register, immediate: 0 });
                        let branch_index = self.output.instructions.len();
                        self.output.instructions.push(Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: 0 });
                        self.evaluate_tail(hot, function.return_type, result)?;
                        self.output.instructions.push(Instruction::BranchToLinkRegister);
                        let cold_label = self.output.instructions.len();
                        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                            *target = cold_label;
                        }
                        self.evaluate_tail(cold, function.return_type, result)?;
                        self.output.instructions.push(Instruction::BranchToLinkRegister);
                        return Ok(());
                    }
                }
                let select = guard_select(&guard.condition, &guard.value, return_expression);
                // ATTEMPT the select; a fall-through outside its vocabulary (a
                // table load, a cast) uses mwcc's early-return BRANCH instead
                // (measured) — roll back and take the guard-sequence path.
                let instructions_before = self.output.instructions.len();
                let relocations_before = self.output.relocations.len();
                let virtuals_before = self.next_virtual;
                let bump_before = self.output.anonymous_label_bump;
                match self.evaluate_tail(&select, function.return_type, result) {
                    Ok(()) => {
                        self.output.instructions.push(Instruction::BranchToLinkRegister);
                        return Ok(());
                    }
                    Err(_) => {
                        self.output.instructions.truncate(instructions_before);
                        self.output.relocations.truncate(relocations_before);
                        self.next_virtual = virtuals_before;
                        self.output.anonymous_label_bump = bump_before;
                    }
                }
            }
            return self.emit_guard_sequence(&function.guards, return_expression, function.return_type, result);
        }

        // The FLOAT DAG arm claims double multiply-add trees (including
        // named double locals — the window-top tier) for the frozen float
        // models before the single-scratch evaluator paths.
        if !self.try_float_dag_return(function)? {
            match function.locals.as_slice() {
                [] => self.evaluate_tail(return_expression, function.return_type, result)?,
                [local] => self.evaluate_single_local(local, return_expression, function.return_type, result)?,
                _ => return Err(Diagnostic::error("multiple locals need the full register allocator (roadmap M1)")),
            }
        }
        // A return value that is itself a call (`return h(p->a, p->b);`) emits its
        // argument setup here, after the body loop's hoist ran — so hoist again now.
        self.hoist_leading_arg_moves(lr_store_index);
        // A `float` function returning a double-precision value rounds to single
        // (`frsp`) before returning, as mwcc does.
        if function.return_type == Type::Float && self.is_double_value(return_expression) {
            self.output.instructions.push(Instruction::RoundToSingle { d: result, b: result });
        }
        self.emit_epilogue_and_return();
        Ok(())
    }

    /// mwcc fills the non-leaf prologue's `mflr`->LR-store latency with the leading
    /// run of register-ALU argument setup — parameter copies / derivations ready at
    /// entry: `stwu; mflr r0; mr r4,r3; mr r5,r3; stw r0,20(r1)`. A register move
    /// (`mr`/logical) or a register `addi` qualifies; an immediate load (`li`,
    /// `addi rD,0,imm`) and memory loads do not, and nothing touching r0 (which the
    /// LR store needs). The slot holds at most two, so the rest stay after the store.
    /// `lr_store_index` is the LR-store's position (only the general non-leaf path
    /// sets it; other paths pass `None` and this is a no-op).
    fn hoist_leading_arg_moves(&mut self, lr_store_index: Option<usize>) {
        let Some(store) = lr_store_index else { return };
        let mut run = 0;
        // A `li`-form argument (`addi rD,0,n`, `a == 0`) is hoisted by the saved-LR-store
        // scheduler when it leads — but once a register move (the indirect-call `mr
        // r12,fp`) has been hoisted ahead of the save, that scheduler can no longer find
        // the save at `mflr+1`, so the `li` must come along here. Allow it only after a
        // move, leaving the lone-`li` direct-call case to the other pass unchanged.
        let mut saw_move = false;
        // mwcc hoists at most the first TWO leading argument-setup instructions into the mflr->LR-store
        // gap (three moves, `sink3(a,b,c)`, keep the third after the store), so the run is capped at 2.
        while run < 2 {
            let Some(instruction) = self.output.instructions.get(store + 1 + run) else { break };
            let hoistable = match *instruction {
                Instruction::Or { a, s, b } => {
                    let movable = a != 0 && s != 0 && b != 0;
                    saw_move |= movable;
                    movable
                }
                Instruction::AddImmediate { d, a, .. } => d != 0 && (a != 0 || saw_move),
                // Any other single-cycle ALU arg-compute (`add`, `mullw`, `subf`, `and`, `xor`, shifts,
                // `neg`) leading a call's argument setup is hoisted the same way (`g(a+b)` ->
                // `add r3,r3,r4; stw r0`). A LOAD arg (`g(*p)`) is NOT hoisted — it stays after the LR
                // save — so this is an ALU whitelist; the no-r0-operand check keeps the hoisted compute
                // independent of the saved-LR store (which reads r0).
                ref other
                    if matches!(other,
                        Instruction::Add { .. } | Instruction::MultiplyLow { .. } | Instruction::SubtractFrom { .. }
                        | Instruction::And { .. } | Instruction::Xor { .. } | Instruction::ShiftLeftWord { .. }
                        | Instruction::ShiftRightWord { .. } | Instruction::ShiftRightAlgebraicWord { .. }
                        | Instruction::Negate { .. } | Instruction::ShiftLeftImmediate { .. }
                        | Instruction::ShiftRightAlgebraicImmediate { .. } | Instruction::ShiftRightLogicalImmediate { .. }
                        | Instruction::ClearLeftImmediate { .. } | Instruction::AndContiguousMask { .. }
                        | Instruction::RotateAndMask { .. } | Instruction::OrImmediate { .. }) =>
                {
                    let movable = mwcc_vreg::register_operands(other).iter().all(|operand| operand.register != 0);
                    saw_move |= movable;
                    movable
                }
                _ => false,
            };
            if !hoistable {
                break;
            }
            run += 1;
        }
        if run > 0 {
            self.output.instructions[store..=store + run].rotate_left(1);
        }
    }

    /// A leaf `void` body that is purely constant stores: mwcc materializes a
    /// repeated store value once and reuses the register (`li r0,0; stw; stw; stw`
    /// for struct/array zeroing). A run of *differing* constants instead needs the
    /// instruction scheduler (distinct registers, interleaved) — defer rather than
    /// emit the unscheduled form. Returns `false` (use the normal path) for bodies
    /// outside this shape, e.g. stores of register-resident values, which already
    /// match.
    /// `T y; if (c) y = A; else y = B; return y;` — both arms assign the same local,
    /// which is then returned, so the body is the select `return (c) ? A : B`. mwcc
    /// compiles it identically to `if (c) return A; return B`. A call in the body
    /// (value live across a branch) is the keystone's and defers.
    pub(crate) fn try_conditional_assign(&mut self, function: &Function) -> Compilation<bool> {
        let [local] = function.locals.as_slice() else { return Ok(false) };
        // An initializer is DEAD here — both arms reassign the local before it is read (verified
        // below) and the handler builds the select purely from the arm values — so allow it:
        // `int b = INIT; if (c) b = A; else b = B; return b;` is the same select as the no-init form,
        // which mwcc compiles identically. (No-else keeps deferring to the initialized handler.)
        if local.array_length.is_some() || !function.guards.is_empty() || function_makes_call(function) {
            return Ok(false);
        }
        let returned = match &function.return_expression {
            Some(Expression::Variable(name)) => name,
            _ => return Ok(false),
        };
        if returned != &local.name {
            return Ok(false);
        }
        let [Statement::If { condition, then_body, else_body }] = function.statements.as_slice() else {
            return Ok(false);
        };
        // Each arm must be exactly `y = <value>` for the returned local `y`.
        let arm_value = |body: &[Statement]| match body {
            [Statement::Assign { name, value }] if name == &local.name => Some(value.clone()),
            _ => None,
        };
        let (Some(when_true), Some(when_false)) = (arm_value(then_body), arm_value(else_body)) else {
            return Ok(false);
        };
        // guard_select's early-return / in-place layout matches mwcc only when the fall-through
        // (else) arm is itself a leaf. With an initializer present, a LEAF then-arm and a COMPUTED
        // else-arm (`int y=a; if(c) y=b; else y=a+1;`) drive mwcc to a SCRATCH-select
        // (`<test>; <else into r0>; b<!c>; <then into r0>; mr result,r0`) that this path does not
        // reproduce — it would emit the conditional-return form and ship wrong bytes. Defer that
        // exact shape (the no-initializer variant already defers downstream).
        let arm_is_leaf = |expr: &Expression| leaf_name(expr).is_some() || constant_value(expr).is_some();
        if local.initializer.is_some() && arm_is_leaf(&when_true) && !arm_is_leaf(&when_false) {
            return Ok(false);
        }
        let result = match function.return_type {
            Type::Float | Type::Double => Eabi::float_result().number,
            _ => Eabi::general_result().number,
        };
        // `if (c) y = A; else y = B;` is the guard `if (c) y = A` with fall-through B
        // — mwcc normalizes a negated `if (!c)` the same way it does a guard return
        // (keep A as the in-place default, strip the `!`), so route through
        // guard_select rather than a bare `(c) ? A : B` select.
        let select = guard_select(condition, &when_true, &when_false);
        self.evaluate_tail(&select, function.return_type, result)?;
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// `T y = INIT; if (c) y = NEW; return y;` (an `if` with no else) where INIT and NEW are
    /// constants. mwcc lowers this conditional ASSIGN as an early-return branch — distinct from the
    /// select/branchless idiom it uses for the equivalent guard `if(c) return NEW; return INIT;`:
    /// `<test c>; li result,INIT; b<!c>lr; li result,NEW; blr` (the false path returns the
    /// initializer already in the result; the true path falls through to the new value). Variable
    /// arms use a different move/staging form and are deferred here.
    pub(crate) fn try_conditional_assign_initialized(&mut self, function: &Function) -> Compilation<bool> {
        let [local] = function.locals.as_slice() else { return Ok(false) };
        let Some(initializer) = &local.initializer else { return Ok(false) };
        if local.array_length.is_some() || !function.guards.is_empty() || function_makes_call(function) {
            return Ok(false);
        }
        let Some(Expression::Variable(returned)) = &function.return_expression else { return Ok(false) };
        if returned != &local.name {
            return Ok(false);
        }
        let [Statement::If { condition, then_body, else_body }] = function.statements.as_slice() else {
            return Ok(false);
        };
        if !else_body.is_empty() {
            return Ok(false);
        }
        let [Statement::Assign { name, value }] = then_body.as_slice() else {
            return Ok(false);
        };
        if name != &local.name {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let result = Eabi::general_result().number;
        let init_const = constant_value(initializer);
        let new_const = constant_value(value);

        // Resolve the variable arms' registers BEFORE emitting the compare (so a deferral leaves no
        // orphaned instructions). Each variable arm must be a leaf already in a register. The MOVE
        // form stages the initializer in a register (the scratch for a constant init, else the init
        // variable's own register); that staged register must differ from the result — mwcc uses a
        // different layout when the init variable already sits in the result — so defer that case.
        let new_register = match new_const {
            Some(_) => None,
            None => match leaf_name(value).and_then(|name| self.lookup_general(name)) {
                register @ Some(_) => register,
                None => return Ok(false),
            },
        };
        let stage = if init_const.is_some() && new_const.is_some() {
            None // both constant -> branch form, no staging register
        } else {
            let stage = match init_const {
                Some(_) => GENERAL_SCRATCH,
                None => match leaf_name(initializer).and_then(|name| self.lookup_general(name)) {
                    Some(register) => register,
                    None => return Ok(false),
                },
            };
            if stage == result {
                return Ok(false);
            }
            Some(stage)
        };

        // emit_condition_test returns the branch-if-FALSE options (a guard's forward-skip sense),
        // which is exactly the early-return / forward-skip-on-!c we want.
        let (options, condition_bit) = self.emit_condition_test(condition)?;

        // Both arms constant: the early-return BRANCH form — return the initializer in place when
        // the condition does not hold, then fall through to the new value.
        let Some(stage) = stage else {
            self.load_integer_constant(result, init_const.unwrap());
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
            self.load_integer_constant(result, new_const.unwrap());
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            return Ok(true);
        };

        // A variable arm: the MOVE/staging form.
        if let Some(init_value) = init_const {
            self.load_integer_constant(stage, init_value);
        }
        let branch_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
        match new_register {
            Some(register) => self.output.instructions.push(Instruction::move_register(stage, register)),
            None => self.load_integer_constant(stage, new_const.unwrap()),
        }
        let after = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
            *target = after;
        }
        self.output.instructions.push(Instruction::move_register(result, stage));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// `T y = v; if (c) y = NEW; return y;` (no else) where the initializer `v` is a variable
    /// ALREADY resident in the result register — the param-0 min/max/abs/clamp idiom. mwcc keeps
    /// the initializer in the result register (no move), tests the condition, and issues a
    /// conditional RETURN on the inverse (`b<!c>lr`) that returns the initializer in place; the
    /// taken path falls through to `<NEW into result>; blr`. Every observed NEW shape — `neg`,
    /// `mr` (a variable), `li` (a constant), `add` (a computed value) — is exactly what the general
    /// tail evaluator emits into the result register, so route NEW through it rather than
    /// re-deriving a per-shape layout. This fills the `stage == result` case the initialized
    /// handler above defers (init already in the result register). Only emits after the last
    /// deferral check, so a deferred NEW (an Err from the evaluator) fails the whole function
    /// rather than leaving orphaned instructions.
    pub(crate) fn try_conditional_overwrite_inplace(&mut self, function: &Function) -> Compilation<bool> {
        let [local] = function.locals.as_slice() else { return Ok(false) };
        if local.array_length.is_some() || !function.guards.is_empty() || function_makes_call(function) {
            return Ok(false);
        }
        // Match the initialized handler's scope: the branch-with-conditional-return form is the
        // int lowering; other widths/types use different staging, so defer them.
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        // The initializer must be a plain variable already living in the result register — then
        // materializing it costs no instruction and the condition test reads it in place. A
        // constant / elsewhere-resident / computed initializer is a different layout (left to the
        // initialized handler or beyond).
        let Some(Expression::Variable(init_name)) = &local.initializer else { return Ok(false) };
        let result = Eabi::general_result().number;
        if self.lookup_general(init_name) != Some(result) {
            return Ok(false);
        }
        // The whole body is `if (c) y = NEW;` (no else) returning y.
        let Some(Expression::Variable(returned)) = &function.return_expression else { return Ok(false) };
        if returned != &local.name {
            return Ok(false);
        }
        let [Statement::If { condition, then_body, else_body }] = function.statements.as_slice() else {
            return Ok(false);
        };
        if !else_body.is_empty() {
            return Ok(false);
        }
        let [Statement::Assign { name, value }] = then_body.as_slice() else {
            return Ok(false);
        };
        if name != &local.name {
            return Ok(false);
        }
        // <test c> — emit_condition_test returns the branch-if-FALSE options (a guard's
        // forward-skip / early-return-on-!c sense), which is exactly what we want here.
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        // b<!c>lr — return the initializer, already in the result register, when c is false.
        self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
        // The taken path computes NEW into the result register, then returns.
        self.evaluate_tail(value, function.return_type, result)?;
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// A PARAMETER conditionally reassigned (optionally after one global store), then
    /// returned: `if (c) { [g = leaf;] [v = NEW;] } return v;`. mwcc keeps v in its
    /// incoming register through the diamond; the skip branch targets the merge, and the
    /// merge is `mr r3,v` — or NOTHING when v already lives in r3, in which case the skip
    /// branch folds to `b<!c>lr` (the conditional-return fold). Captured shapes, GC/2.6:
    ///   `if (a<b) a=b; return a;`        -> cmpw; bgelr; mr r3,r4; blr
    ///   `if (a<b) b=b+1; return b;`      -> cmpw; bge M; addi r4,r4,1; M: mr r3,r4; blr
    ///   `if (a>0) { g=a; a=a-1; } ret a` -> cmpwi; blelr; stw r3; addi r3,r3,-1; blr
    ///   `if (a>0) { g=a; } return a;`    -> cmpwi; blelr; stw r3; blr
    /// LONGER then-bodies RESCHEDULE (a second store sinks below the addi — measured), so
    /// only the probed [Store], [Assign], [Store, Assign] forms are taken; more defers.
    pub(crate) fn try_conditional_reassign_return(&mut self, function: &Function) -> Compilation<bool> {
        if !function.guards.is_empty() || !function.locals.is_empty() || function_makes_call(function) {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let Some(Expression::Variable(returned)) = &function.return_expression else { return Ok(false) };
        let [Statement::If { condition, then_body, else_body }] = function.statements.as_slice() else {
            return Ok(false);
        };
        let Some(location) = self.locations.get(returned.as_str()) else { return Ok(false) };
        if location.class != ValueClass::General || location.width != 32 {
            return Ok(false);
        }
        let home = location.register;
        let result = Eabi::general_result().number;
        // No side effect in either arm of an if/ELSE: the SELECT layouts — checked
        // before the reassign plan, whose in-place gates are narrower than select's
        // computed-from-any-register arms.
        if !else_body.is_empty()
            && !then_body.iter().chain(else_body.iter()).any(|statement| matches!(statement, Statement::Store { .. }))
        {
            return self.try_select_diamond(condition, then_body, else_body, returned);
        }
        let Some(then_order) = self.conditional_reassign_plan(then_body, returned) else { return Ok(false) };

        if else_body.is_empty() {
            // SINGLE-SIDED: v keeps its incoming register; the merge is `mr r3,v`, empty
            // (and folded to a conditional return) when v already lives in r3.
            // -- commit (an Err past here defers the whole function; never Ok(false)) --
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            let merge = if home == result { None } else { Some(self.fresh_label()) };
            match merge {
                Some(label) => self.emit_branch_conditional_to(options, condition_bit, label),
                None => self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit }),
            }
            self.emit_conditional_reassign_body(&then_order, home)?;
            if let Some(label) = merge {
                self.bind_label(label);
                self.output.instructions.push(Instruction::move_register(result, home));
            }
            self.emit_epilogue_and_return();
            return Ok(true);
        }

        let Some(else_order) = self.conditional_reassign_plan(else_body, returned) else { return Ok(false) };
        let then_ends_assign = matches!(then_body.last(), Some(Statement::Assign { .. }));
        let else_ends_assign = matches!(else_body.last(), Some(Statement::Assign { .. }));

        if then_ends_assign && else_ends_assign {
            // ARM-EXIT: both arms rewrite v last, so each arm computes the RETURN VALUE
            // directly into r3 and returns — no merge, no re-test (measured: `addi
            // r3,r4,1; blr` / an else of `b=a` with a in r3 emits NOTHING, its branch
            // folding to `b<c>lr`). Two statements per arm at most: a THREE-statement
            // arm takes the working-register diamond (through r0, an unconditional
            // branch to a shared `mr r3,r0` merge — measured on x6) — deferred.
            if then_body.len() > 2 || else_body.len() > 2 {
                return Ok(false);
            }
            let then_empty = self.reassign_arm_is_empty(&then_order, result);
            let else_empty = self.reassign_arm_is_empty(&else_order, result);
            if then_empty && else_empty {
                return Ok(false);
            }
            // -- commit --
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            if else_empty {
                // The else returns v unchanged (already r3): branch-to-LR on !c.
                self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
                self.emit_reassign_arm_into_result(&then_order, home, result)?;
            } else if then_empty {
                // The mirror: return unchanged on c, fall into the else arm.
                self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: options ^ 8, condition_bit });
                self.emit_reassign_arm_into_result(&else_order, home, result)?;
            } else {
                let else_label = self.fresh_label();
                self.emit_branch_conditional_to(options, condition_bit, else_label);
                self.emit_reassign_arm_into_result(&then_order, home, result)?;
                self.emit_epilogue_and_return();
                self.bind_label(else_label);
                self.emit_reassign_arm_into_result(&else_order, home, result)?;
            }
            self.emit_epilogue_and_return();
            return Ok(true);
        }

        // RE-TEST SPLIT: two independent guards — the then-arm, then the same compare
        // RE-EMITTED with the branch sense inverted for the else-arm; the second guard
        // folds to a conditional return when the merge is empty (the single-sided rules).
        // -- commit --
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let skip_then = self.fresh_label();
        self.emit_branch_conditional_to(options, condition_bit, skip_then);
        self.emit_conditional_reassign_body(&then_order, home)?;
        self.bind_label(skip_then);
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let inverted = options ^ 8;
        let merge = if home == result { None } else { Some(self.fresh_label()) };
        match merge {
            Some(label) => self.emit_branch_conditional_to(inverted, condition_bit, label),
            None => self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: inverted, condition_bit }),
        }
        self.emit_conditional_reassign_body(&else_order, home)?;
        if let Some(label) = merge {
            self.bind_label(label);
            self.output.instructions.push(Instruction::move_register(result, home));
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// True when an arm emits no code: no stores, and its reassignment is a copy whose
    /// source already lives in the result register.
    fn reassign_arm_is_empty(&self, order: &[&Statement], result: u8) -> bool {
        order.iter().all(|statement| match statement {
            Statement::Assign { value: Expression::Variable(source), .. } => self.lookup_general(source) == Some(result),
            _ => false,
        })
    }

    /// Emit one arm-exit arm: stores, then the final reassignment computed DIRECTLY into
    /// the result register (`mr r3,w` elided when w is r3; `addi r3,v,±C`; `li r3,C`).
    fn emit_reassign_arm_into_result(&mut self, order: &[&Statement], home: u8, result: u8) -> Compilation<()> {
        for statement in order {
            match statement {
                Statement::Store { target, value } => self.emit_store(target, value)?,
                Statement::Assign { value, .. } => match value {
                    Expression::Variable(source) => {
                        let source = self.lookup_general(source).expect("gated: register-resident");
                        if source != result {
                            self.output.instructions.push(Instruction::move_register(result, source));
                        }
                    }
                    Expression::Binary { operator, right, .. } => {
                        let constant = constant_value(right).expect("gated: i16 constant") as i16;
                        let immediate = if *operator == BinaryOperator::Subtract { -constant } else { constant };
                        self.output.instructions.push(Instruction::AddImmediate { d: result, a: home, immediate });
                    }
                    other => {
                        let constant = constant_value(other).expect("gated: i16 constant") as i16;
                        self.output.instructions.push(Instruction::load_immediate(result, constant));
                    }
                },
                _ => unreachable!("gated"),
            }
        }
        Ok(())
    }

    /// A pure-assign diamond — `if (c) v = X; else v = Y; return v;` with no side
    /// effects — takes mwcc's SELECT layouts (measured, ten boundary probes):
    ///
    /// A CONSTANT arm is SPECULATED into the phi register in the compare latency slot
    /// (both constant: the else), the branch skipping the other (conditional) arm; with
    /// no constant, a COPY else COALESCES — phi becomes the copy's source register and
    /// the else emits nothing; otherwise the else speculates. The phi is r3 itself when
    /// the conditional arm does not read r3 (merge elided, the branch folding to
    /// b<c>lr), else r0; a coalesced phi is wherever the else source lives. The merge,
    /// when present, is `mr r3,phi`.
    fn try_select_diamond(&mut self, condition: &Expression, then_body: &[Statement], else_body: &[Statement], returned: &str) -> Compilation<bool> {
        let Some(then_arm) = self.classify_select_arm(then_body, returned) else { return Ok(false) };
        let Some(else_arm) = self.classify_select_arm(else_body, returned) else { return Ok(false) };
        let result = Eabi::general_result().number;
        let then_const = matches!(then_arm, SelectArm::Constant(_));
        let else_const = matches!(else_arm, SelectArm::Constant(_));

        if !then_const && !else_const {
            if let SelectArm::Copy(phi) = else_arm {
                // COALESCE: the else vanishes; the then-arm computes into phi.
                if matches!(then_arm, SelectArm::Copy(source) if source == phi) {
                    return Ok(false); // a self-move then-arm is unprobed
                }
                // -- commit --
                let (options, condition_bit) = self.emit_condition_test(condition)?;
                if phi == result {
                    self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
                    self.emit_select_arm(&then_arm, phi);
                } else {
                    let merge = self.fresh_label();
                    self.emit_branch_conditional_to(options, condition_bit, merge);
                    self.emit_select_arm(&then_arm, phi);
                    self.bind_label(merge);
                    self.output.instructions.push(Instruction::move_register(result, phi));
                }
                self.emit_epilogue_and_return();
                return Ok(true);
            }
        }

        // SPECULATE: the constant arm if exactly one (the else when both or neither).
        let (speculated, conditional, conditional_is_then) = if then_const && !else_const {
            (&then_arm, &else_arm, false)
        } else {
            (&else_arm, &then_arm, true)
        };
        let conditional_reads_result = match conditional {
            SelectArm::Copy(source) | SelectArm::Computed { source, .. } => *source == result,
            SelectArm::Constant(_) => false,
        };
        let phi = if conditional_reads_result { GENERAL_SCRATCH } else { result };
        // -- commit --
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        self.emit_select_arm(speculated, phi);
        let skip = if conditional_is_then { options } else { options ^ 8 };
        if phi == result {
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: skip, condition_bit });
            self.emit_select_arm(conditional, phi);
        } else {
            let merge = self.fresh_label();
            self.emit_branch_conditional_to(skip, condition_bit, merge);
            self.emit_select_arm(conditional, phi);
            self.bind_label(merge);
            self.output.instructions.push(Instruction::move_register(result, phi));
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// One select arm as a value: a register copy, a register ± constant, or a constant.
    fn classify_select_arm(&self, body: &[Statement], returned: &str) -> Option<SelectArm> {
        let [Statement::Assign { name, value }] = body else { return None };
        if name.as_str() != returned {
            return None;
        }
        match value {
            Expression::Variable(source) => Some(SelectArm::Copy(self.lookup_general(source)?)),
            Expression::Binary { operator: operator @ (BinaryOperator::Add | BinaryOperator::Subtract), left, right } => {
                let Expression::Variable(source) = left.as_ref() else { return None };
                let source = self.lookup_general(source)?;
                let constant = i16::try_from(constant_value(right)?).ok()?;
                let immediate = if *operator == BinaryOperator::Subtract { -constant } else { constant };
                Some(SelectArm::Computed { source, immediate })
            }
            other => Some(SelectArm::Constant(i16::try_from(constant_value(other)?).ok()?)),
        }
    }

    /// Materialize a select arm into the phi register.
    fn emit_select_arm(&mut self, arm: &SelectArm, phi: u8) {
        match arm {
            SelectArm::Constant(constant) => self.output.instructions.push(Instruction::load_immediate(phi, *constant)),
            SelectArm::Copy(source) => self.output.instructions.push(Instruction::move_register(phi, *source)),
            SelectArm::Computed { source, immediate } => {
                self.output.instructions.push(Instruction::AddImmediate { d: phi, a: *source, immediate: *immediate })
            }
        }
    }

    /// Gate and order one arm of the conditional-reassign form: up to THREE statements
    /// — scalar-global stores of register variables and AT MOST ONE in-place
    /// reassignment of `returned` (`mr` from a register variable, `addi` self-adjust,
    /// or `li` constant) — in source order after the STORE-PAIR BREAK (mwcc pulls a
    /// following reassignment between two adjacent stores; blocked when the jumped
    /// store reads the reassigned variable). A store AFTER a var-copy or constant
    /// reassignment value-forwards the source register instead (measured) — `None`.
    fn conditional_reassign_plan<'a>(&self, body: &'a [Statement], returned: &str) -> Option<Vec<&'a Statement>> {
        if body.is_empty() || body.len() > 3 {
            return None;
        }
        let mut assign_count = 0usize;
        let mut stores_blocked = false;
        for statement in body {
            match statement {
                Statement::Store { target, value } => {
                    if stores_blocked {
                        return None;
                    }
                    let Expression::Variable(global) = target else { return None };
                    if !matches!(self.globals.get(global.as_str()), Some(Type::Int | Type::UnsignedInt)) {
                        return None;
                    }
                    if self.global_array_sizes.contains_key(global.as_str()) {
                        return None;
                    }
                    let Expression::Variable(source) = value else { return None };
                    self.lookup_general(source)?;
                }
                Statement::Assign { name, value } => {
                    if name.as_str() != returned {
                        return None;
                    }
                    assign_count += 1;
                    if assign_count > 1 {
                        return None;
                    }
                    match value {
                        Expression::Variable(source) => {
                            self.lookup_general(source)?;
                            stores_blocked = true;
                        }
                        Expression::Binary { operator: BinaryOperator::Add | BinaryOperator::Subtract, left, right } => {
                            let reads_self = matches!(left.as_ref(), Expression::Variable(source) if source.as_str() == returned);
                            if !reads_self || constant_value(right).and_then(|value| i16::try_from(value).ok()).is_none() {
                                return None;
                            }
                        }
                        other if constant_value(other).and_then(|value| i16::try_from(value).ok()).is_some() => {
                            stores_blocked = true;
                        }
                        _ => return None,
                    }
                }
                _ => return None,
            }
        }
        let mut order: Vec<&Statement> = body.iter().collect();
        for index in 0..order.len().saturating_sub(2) {
            if !matches!((order[index], order[index + 1]), (Statement::Store { .. }, Statement::Store { .. })) {
                continue;
            }
            if matches!(order[index + 2], Statement::Assign { .. }) {
                let Statement::Store { value, .. } = order[index + 1] else { unreachable!() };
                let jumped_reads_v = matches!(value, Expression::Variable(source) if source.as_str() == returned);
                if !jumped_reads_v {
                    order.swap(index + 1, index + 2);
                }
            }
        }
        Some(order)
    }

    /// Emit one planned arm: stores through the store path, reassignments in place.
    fn emit_conditional_reassign_body(&mut self, order: &[&Statement], home: u8) -> Compilation<()> {
        for statement in order {
            match statement {
                Statement::Store { target, value } => self.emit_store(target, value)?,
                Statement::Assign { value, .. } => match value {
                    Expression::Variable(source) => {
                        let source = self.lookup_general(source).expect("gated: register-resident");
                        self.output.instructions.push(Instruction::move_register(home, source));
                    }
                    Expression::Binary { operator, right, .. } => {
                        let constant = constant_value(right).expect("gated: i16 constant") as i16;
                        let immediate = if *operator == BinaryOperator::Subtract { -constant } else { constant };
                        self.output.instructions.push(Instruction::AddImmediate { d: home, a: home, immediate });
                    }
                    other => {
                        let constant = constant_value(other).expect("gated: i16 constant") as i16;
                        self.output.instructions.push(Instruction::load_immediate(home, constant));
                    }
                },
                _ => unreachable!("gated"),
            }
        }
        Ok(())
    }

    pub(crate) fn try_constant_store_fill(&mut self, function: &Function) -> Compilation<bool> {
        if function_makes_call(function)
            || function.return_type != Type::Void
            || !function.guards.is_empty()
            || !function.locals.is_empty()
        {
            return Ok(false);
        }
        // The store run is either the whole body, or the body of a single trailing `if (c) { … }`
        // with no else — the same batched constant materialization, wrapped in a conditional return
        // (`cmpwi;beqlr; <run>`). Everything below (detection, register plan) works on `statements`;
        // the conditional-return guard is emitted just before materializing, so a non-run body
        // returns Ok(false) without leaving orphaned instructions.
        let (statements, guard): (&[Statement], Option<&Expression>) = match function.statements.as_slice() {
            [Statement::If { condition, then_body, else_body }] if else_body.is_empty() => (then_body.as_slice(), Some(condition)),
            other => (other, None),
        };
        let Some(plan) = self.constant_store_run_plan(statements) else {
            return Ok(false);
        };
        // Commit. Emit the conditional-return guard first (for the trailing-if form), then the run.
        if let Some(condition) = guard {
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
        }
        self.emit_constant_store_run(statements, plan)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// The register plan for a run of two-or-more constant stores to scratch-safe targets, or
    /// `None` when the statements are not such a run (a non-store, a non-constant value, an unsafe
    /// target) or cannot be scheduled here (3+ distinct constants with a non-global or duplicate
    /// value, or no free register). Pure — used both to emit a run and to pre-check an if-else arm.
    fn constant_store_run_plan(&self, statements: &[Statement]) -> Option<ConstStoreRun> {
        if statements.len() < 2 {
            return None;
        }
        let mut constants = Vec::new();
        for statement in statements {
            let Statement::Store { target, value } = statement else { return None };
            if !self.is_scratch_safe_store_target(target) {
                return None;
            }
            constants.push(constant_value(value)? as i32);
        }
        if constants.iter().all(|constant| *constant == constants[0]) {
            return Some(ConstStoreRun::AllSame);
        }
        if constants.len() == 2 {
            // Two distinct constants: the first into a free register, the second into the scratch.
            let base_registers: Vec<u8> = statements.iter()
                .filter_map(|statement| match statement {
                    Statement::Store { target, .. } => self.store_base_register(target),
                    _ => None,
                })
                .collect();
            let first_register = (3u8..=12).find(|r| *r != GENERAL_SCRATCH && !base_registers.contains(r) && !self.reserved.contains(r))?;
            return Some(ConstStoreRun::Distinct(vec![(constants[0], first_register), (constants[1], GENERAL_SCRATCH)]));
        }
        // 3+ distinct constants to small-data globals: r(N+1) descending to r3 and the last into r0.
        // Member/dereference targets reschedule with their base register, and a duplicate constant
        // shares one register — both fall out of this plan.
        let all_globals = statements.iter().all(|statement| {
            matches!(statement, Statement::Store { target: Expression::Variable(_), .. })
        });
        let count = constants.len();
        let mut distinct = constants.clone();
        distinct.sort_unstable();
        distinct.dedup();
        if !all_globals || distinct.len() != count || count + 1 > 12 {
            return None;
        }
        let assignments = constants.iter().enumerate().map(|(index, &constant)| {
            let register = if index + 1 < count { (count + 1 - index) as u8 } else { GENERAL_SCRATCH };
            (constant, register)
        }).collect();
        Some(ConstStoreRun::Distinct(assignments))
    }

    /// Emit a planned constant store run: materialize the values (all up front for `Distinct`, or
    /// once into the reused scratch for `AllSame`), then the stores in source order. No epilogue.
    fn emit_constant_store_run(&mut self, statements: &[Statement], plan: ConstStoreRun) -> Compilation<()> {
        match plan {
            ConstStoreRun::Distinct(assignments) => {
                for &(constant, register) in &assignments {
                    self.load_integer_constant(register, constant as i64);
                }
                self.prematerialized_constants = assignments;
                for statement in statements {
                    self.emit_statement(statement)?;
                }
                self.prematerialized_constants.clear();
            }
            ConstStoreRun::AllSame => {
                self.reuse_scratch_constant = true;
                self.scratch_constant = None;
                for statement in statements {
                    self.emit_statement(statement)?;
                }
                self.reuse_scratch_constant = false;
                self.scratch_constant = None;
            }
        }
        Ok(())
    }

    /// Whether an if-else arm is a run of two-plus REGISTER-VALUED stores (each value a param/local
    /// already in a register) — emitted sequentially, no value to materialize.
    fn store_run_arm_registers(&self, statements: &[Statement]) -> bool {
        statements.len() >= 2 && statements.iter().all(|statement| matches!(statement,
            Statement::Store { value: Expression::Variable(name), .. } if self.locations.contains_key(name.as_str())))
    }

    /// A whole-body `if (c) { <store run> } else { <store run> }` where each arm is two-plus stores
    /// whose values are either all REGISTER-valued (emitted sequentially) or all CONSTANT (the
    /// batched materialization): `cmpwi; beq else; <then run>; blr; else: <else run>; blr`. The
    /// no-else form is handled by try_constant_store_fill / the register-valued trailing-if path.
    /// THE PUNNED-GUARD WRITEBACK (the s_floor tail, fire 380): punned int
    /// reads of the double param spill it to the frame, a guard block
    /// mutates the punned locals in scratch registers, the block writes
    /// them back and the double reloads. Measured (one and two locals):
    /// stwu; cmpwi (HOISTED — the second local reuses the freed condition
    /// register); stfd f1,8; lwz r0[, lwz r3]; beq JOIN; li...; JOIN:
    /// stw...; lfd f1,8; addi; blr.
    /// The BRANCHLESS ZERO-SELECT: `if (j0 cmp K) p = A; else p = B;` with
    /// one arm 0 if-converts to mask algebra — no branches (measured
    /// L3/L4/S2/S3/R1/R2/R3 on 2.6). The mask is -(cond); zero-in-then
    /// selects with andc (else & ~mask), zero-in-else with and. Recipes:
    ///   <  : li rK; srwi sign(K); subfc K,g; srwi sign(g); subfe
    ///   >  : the swapped form (rK/sign registers trade places)
    ///   == : addi g-K; subfic K-g; nor; srawi 31
    ///   != : the same with or
    ///   <= : xoris 0x8000; subfic; addc; subfe rM,rM,rM
    /// Registers: the select home is r0; </> put K,sign in r3/r4 and the
    /// load in r5; ==/!=/<= compute in place on the r3 load. The L4
    /// self-mask arm (`p &= M`) keeps the load in r0 and weaves rlwinm
    /// between the guard extract and its -K addi.
    fn try_punned_zero_select(
        &mut self,
        locals: &[(&str, i16)],
        guard: &GuardLocal,
        condition: &Expression,
        then_body: &[Statement],
        else_body: &[Statement],
    ) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        let punned = locals[0].0;
        let offset = locals[0].1;
        let Expression::Binary { operator, left, right } = condition else {
            return Ok(false);
        };
        let operator = *operator;
        if !matches!(
            operator,
            BinaryOperator::Less
                | BinaryOperator::Greater
                | BinaryOperator::Equal
                | BinaryOperator::NotEqual
                | BinaryOperator::LessEqual
        ) {
            return Ok(false);
        }
        if !matches!(left.as_ref(), Expression::Variable(name) if name == guard.name) {
            return Ok(false);
        }
        let Some(bound) = crate::analysis::constant_value(right) else {
            return Ok(false);
        };
        let Ok(bound) = i16::try_from(bound) else {
            return Ok(false);
        };
        let ([Statement::Assign { name: then_name, value: then_value }], [Statement::Assign { name: else_name, value: else_value }]) =
            (then_body, else_body)
        else {
            return Ok(false);
        };
        if then_name != punned || else_name != punned {
            return Ok(false);
        }
        let then_zero = crate::analysis::constant_value(then_value) == Some(0);
        let else_zero = crate::analysis::constant_value(else_value) == Some(0);
        let (live_value, select_complement) = match (then_zero, else_zero) {
            (true, false) => (else_value, true),  // else & ~mask
            (false, true) => (then_value, false), // then & mask
            _ => return Ok(false),
        };
        // The live arm: a small constant, or the measured L4 self-mask
        // (`p & M`, only captured under `<` with the zero in the then).
        enum LiveArm {
            Constant(i16),
            SelfMask { begin: u8, end: u8 },
        }
        let live_arm = if let Some(constant) = crate::analysis::constant_value(live_value) {
            let Ok(small) = i16::try_from(constant) else {
                return Ok(false);
            };
            LiveArm::Constant(small)
        } else if let Expression::Binary { operator: BinaryOperator::BitAnd, left, right } = live_value {
            if !(operator == BinaryOperator::Less && select_complement) {
                return Ok(false);
            }
            if !matches!(left.as_ref(), Expression::Variable(name) if name == punned) {
                return Ok(false);
            }
            let Some((begin, end)) =
                crate::analysis::constant_value(right).and_then(crate::analysis::rlwinm_mask)
            else {
                return Ok(false);
            };
            LiveArm::SelfMask { begin, end }
        } else {
            return Ok(false);
        };
        // The guard is read by the condition alone; the arms touch only p.
        if count_name_occurrences(condition, guard.name) != 1
            || count_name_occurrences(then_value, guard.name) != 0
            || count_name_occurrences(else_value, guard.name) != 0
        {
            return Ok(false);
        }
        let offset_negative = if guard.offset_k != 0 {
            let Ok(negative) = i16::try_from(-guard.offset_k) else {
                return Ok(false);
            };
            Some(negative)
        } else {
            None
        };
        // -- commit --
        let self_mask_arm = matches!(live_arm, LiveArm::SelfMask { .. });
        let carry_form = matches!(operator, BinaryOperator::Less | BinaryOperator::Greater);
        // Homes: the select value in r0; </> claim r3/r4 for K and its
        // sign; the load lands beyond them (r5) or shares r0 (self-mask).
        let load_register: u8 = if self_mask_arm {
            0
        } else if carry_form {
            5
        } else {
            3
        };
        let guard_register: u8 = if carry_form { 5 } else { 3 };
        self.frame_size = 16;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        match operator {
            BinaryOperator::Less => {
                self.output.instructions.push(Instruction::load_immediate(3, bound));
                self.output.instructions.push(Instruction::RotateAndMask { a: 4, s: 3, shift: 1, begin: 31, end: 31 });
            }
            BinaryOperator::Greater => {
                self.output.instructions.push(Instruction::load_immediate(4, bound));
                self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 4, shift: 1, begin: 31, end: 31 });
            }
            _ => {}
        }
        if let LiveArm::Constant(constant) = live_arm {
            self.output.instructions.push(Instruction::load_immediate(0, constant));
        }
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: load_register, a: 1, offset: 8 + offset });
        match guard.mask {
            Some(mask) => {
                let rotated = ((32 - guard.shift as u32) % 32) as u8;
                let Some((begin, end)) = crate::analysis::rlwinm_mask(mask) else {
                    return Err(Diagnostic::error("guard mask is not a run (roadmap)"));
                };
                self.output.instructions.push(Instruction::RotateAndMask {
                    a: guard_register,
                    s: load_register,
                    shift: rotated,
                    begin,
                    end,
                });
            }
            None => {
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate {
                    a: guard_register,
                    s: load_register,
                    shift: guard.shift,
                });
            }
        }
        if let LiveArm::SelfMask { begin, end } = live_arm {
            // The arm rlwinm weaves between the guard extract and its addi
            // (measured L4).
            self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 0, shift: 0, begin, end });
        }
        if let Some(negative) = offset_negative {
            self.output.instructions.push(Instruction::AddImmediate {
                d: guard_register,
                a: guard_register,
                immediate: negative,
            });
        }
        let g = guard_register;
        match operator {
            BinaryOperator::Less => {
                self.output.instructions.push(Instruction::SubtractFromCarrying { d: 3, a: 3, b: g });
                self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: g, shift: 1, begin: 31, end: 31 });
                self.output.instructions.push(Instruction::SubtractFromExtended { d: 3, a: 3, b: 4 });
            }
            BinaryOperator::Greater => {
                self.output.instructions.push(Instruction::SubtractFromCarrying { d: 4, a: g, b: 4 });
                self.output.instructions.push(Instruction::RotateAndMask { a: 4, s: g, shift: 1, begin: 31, end: 31 });
                self.output.instructions.push(Instruction::SubtractFromExtended { d: 3, a: 3, b: 4 });
            }
            BinaryOperator::Equal | BinaryOperator::NotEqual => {
                let Ok(negated) = i16::try_from(-(bound as i32)) else {
                    return Err(Diagnostic::error("select bound beyond i16 (roadmap)"));
                };
                self.output.instructions.push(Instruction::AddImmediate { d: 4, a: g, immediate: negated });
                self.output.instructions.push(Instruction::SubtractFromImmediate { d: 3, a: g, immediate: bound });
                if operator == BinaryOperator::Equal {
                    self.output.instructions.push(Instruction::Nor { a: 3, s: 4, b: 3 });
                } else {
                    self.output.instructions.push(Instruction::Or { a: 3, s: 4, b: 3 });
                }
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate { a: 3, s: 3, shift: 31 });
            }
            BinaryOperator::LessEqual => {
                self.output.instructions.push(Instruction::XorImmediateShifted { a: 4, s: g, immediate: 0x8000 });
                self.output.instructions.push(Instruction::SubtractFromImmediate { d: 3, a: g, immediate: bound });
                self.output.instructions.push(Instruction::AddCarrying { d: 3, a: 3, b: 4 });
                self.output.instructions.push(Instruction::SubtractFromExtended { d: 3, a: 3, b: 3 });
            }
            _ => unreachable!("gated above"),
        }
        if select_complement {
            self.output.instructions.push(Instruction::AndComplement { a: 0, s: 0, b: 3 });
        } else {
            self.output.instructions.push(Instruction::And { a: 0, s: 0, b: 3 });
        }
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 8 + offset });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // The if-converted diamond still costs its labels (measured +3:
        // real @8/@9 vs the +0 base's @5/@6 on the L3 object); the
        // compound self-mask arm adds one more (L4's @9/@10).
        self.output.anonymous_label_bump += if self_mask_arm { 4 } else { 3 };
        Ok(true)
    }

    /// The HOISTED-ELSE OVERWRITE: `if (j0 cmp K) p = C1; else p = C2;`
    /// with BOTH arms nonzero constants branches (no if-conversion) with
    /// the else value pre-loaded into the home before the compare and the
    /// then arm as a skip (measured H1–H7, all six comparison ops):
    ///   li rHome,C2; stfd; lwz r0; extract; [addi r0,-K0]; cmpwi r0;
    ///   b<inverted> skip; li rHome,C1; skip: stw rHome
    /// Homes obey the LIVENESS rule: the pre-loaded else value crosses the
    /// r0 write, so rHome = r4 when the guard holds a home (K0 fold) and
    /// r3 when the extract goes straight to r0 (K0 = 0, H7).
    fn try_punned_hoisted_overwrite(
        &mut self,
        locals: &[(&str, i16)],
        guard: &GuardLocal,
        condition: &Expression,
        then_body: &[Statement],
        else_body: &[Statement],
    ) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        let punned = locals[0].0;
        let offset = locals[0].1;
        let Expression::Binary { operator, left, right } = condition else {
            return Ok(false);
        };
        // The inverted skip branch, (options, condition_bit) per op.
        let inverted = match operator {
            BinaryOperator::Less => (4, 0),          // bge
            BinaryOperator::Greater => (4, 1),       // ble
            BinaryOperator::Equal => (4, 2),         // bne
            BinaryOperator::NotEqual => (12, 2),     // beq
            BinaryOperator::LessEqual => (12, 1),    // bgt
            BinaryOperator::GreaterEqual => (12, 0), // blt
            _ => return Ok(false),
        };
        if !matches!(left.as_ref(), Expression::Variable(name) if name == guard.name) {
            return Ok(false);
        }
        let Some(bound) = crate::analysis::constant_value(right) else {
            return Ok(false);
        };
        let Ok(bound) = i16::try_from(bound) else {
            return Ok(false);
        };
        let ([Statement::Assign { name: then_name, value: then_value }], [Statement::Assign { name: else_name, value: else_value }]) =
            (then_body, else_body)
        else {
            return Ok(false);
        };
        if then_name != punned || else_name != punned {
            return Ok(false);
        }
        let (Some(then_constant), Some(else_constant)) = (
            crate::analysis::constant_value(then_value),
            crate::analysis::constant_value(else_value),
        ) else {
            return Ok(false);
        };
        let (Ok(then_constant), Ok(else_constant)) =
            (i16::try_from(then_constant), i16::try_from(else_constant))
        else {
            return Ok(false);
        };
        if then_constant == 0 || else_constant == 0 {
            // One-zero forms if-convert (the zero-select path claims them
            // first); both-zero is unmeasured.
            return Ok(false);
        }
        if count_name_occurrences(condition, guard.name) != 1 {
            return Ok(false);
        }
        let offset_negative = if guard.offset_k != 0 {
            let Ok(negative) = i16::try_from(-guard.offset_k) else {
                return Ok(false);
            };
            Some(negative)
        } else {
            None
        };
        // -- commit --
        // With the -K0 fold the guard needs a home (r3) and the else value
        // lands beyond it (r4); without it the extract computes in place
        // on r0 and the home is r3 (measured H7).
        let home: u8 = if offset_negative.is_some() { 4 } else { 3 };
        let guard_register: u8 = if offset_negative.is_some() { 3 } else { 0 };
        self.frame_size = 16;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::load_immediate(home, else_constant));
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 8 + offset });
        match guard.mask {
            Some(mask) => {
                let rotated = ((32 - guard.shift as u32) % 32) as u8;
                let Some((begin, end)) = crate::analysis::rlwinm_mask(mask) else {
                    return Err(Diagnostic::error("guard mask is not a run (roadmap)"));
                };
                self.output.instructions.push(Instruction::RotateAndMask {
                    a: guard_register,
                    s: 0,
                    shift: rotated,
                    begin,
                    end,
                });
            }
            None => {
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate {
                    a: guard_register,
                    s: 0,
                    shift: guard.shift,
                });
            }
        }
        if let Some(negative) = offset_negative {
            self.output.instructions.push(Instruction::AddImmediate {
                d: 0,
                a: guard_register,
                immediate: negative,
            });
        }
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: bound });
        let skip = self.fresh_label();
        self.emit_branch_conditional_to(inverted.0, inverted.1, skip);
        self.output.instructions.push(Instruction::load_immediate(home, then_constant));
        self.bind_label(skip);
        self.output.instructions.push(Instruction::StoreWord { s: home, a: 1, offset: 8 + offset });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // The diamond's labels (measured +3 on the H1 object: real @8/@9
        // vs the +0 base's @5/@6 — the same count as the if-converted
        // select, so the label cost predates the conversion decision).
        self.output.anonymous_label_bump += 3;
        Ok(true)
    }

    /// THE MODF LADDER (fire 405): s_modf's three-arm shape — pointer
    /// stores through the second param, the INTEGRAL block (sign-only pun
    /// store into x's spill + f1 reload), and `x - *iptr` (lfd + fsub).
    /// Registers per the capture with r3 = the live pointer param: temp
    /// r4, loads r5/r6, the scrutinee j0 r7; the integral block reuses
    /// the (path-dead) param register r3 as its scratch.
    fn try_punned_modf_ladder(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        if function.return_type != Type::Double
            || !function.guards.is_empty()
            || function_makes_call(function)
            || self.non_leaf
            || function.return_expression.is_some()
        {
            return Ok(false);
        }
        let [x_param, pointer_param] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if x_param.parameter_type != Type::Double
            || pointer_param.parameter_type != Type::Pointer(Pointee::Double)
        {
            return Ok(false);
        }
        let x = x_param.name.as_str();
        let iptr = pointer_param.name.as_str();
        // Locals: initialized punned pair + guard; the uninitialized shift.
        let mut locals: Vec<(&str, i16)> = Vec::new();
        let mut guard: Option<GuardLocal> = None;
        let mut shift: Option<&str> = None;
        for local in &function.locals {
            if local.array_length.is_some() {
                return Ok(false);
            }
            match (&local.initializer, local.declared_type) {
                (Some(init), Type::Int) => {
                    if let Some(offset) = crate::frame::pun_word_offset_pub(init, x) {
                        if locals.iter().any(|&(_, seen)| seen == offset) {
                            return Ok(false);
                        }
                        locals.push((local.name.as_str(), offset));
                    } else if guard.is_none() {
                        let Some(parsed) = parse_guard_init(local.name.as_str(), init) else {
                            return Ok(false);
                        };
                        guard = Some(parsed);
                    } else {
                        return Ok(false);
                    }
                }
                (None, Type::UnsignedInt) if shift.is_none() => shift = Some(local.name.as_str()),
                _ => return Ok(false),
            }
        }
        let (Some(guard), Some(shift)) = (guard, shift) else {
            return Ok(false);
        };
        if locals.len() != 2
            || locals[0].1 != 0
            || locals[1].1 != 4
            || !locals.iter().any(|&(name, _)| name == guard.source)
            || guard.offset_k == 0
            || i16::try_from(-guard.offset_k).is_err()
        {
            return Ok(false);
        }
        let local_index = |name: &str| locals.iter().position(|&(local, _)| local == name);
        let i0 = local_index(guard.source).expect("checked");
        if i0 != 0 {
            return Ok(false); // the high word drives everything
        }
        // The single statement: the outer ladder (every leaf returns).
        let [Statement::If { condition: ladder1, then_body: low_arm, else_body: high_arm }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        let parse_guard_compare = |condition: &Expression, operator: BinaryOperator| -> Option<i16> {
            let Expression::Binary { operator: op, left, right } = condition else { return None };
            if *op != operator || !matches!(left.as_ref(), Expression::Variable(v) if v == guard.name) {
                return None;
            }
            crate::analysis::constant_value(right).and_then(|k| i16::try_from(k).ok())
        };
        let Some(k1) = parse_guard_compare(ladder1, BinaryOperator::Less) else {
            return Ok(false);
        };
        let [Statement::If { condition: split, then_body: arm1, else_body: arm2 }] = low_arm.as_slice()
        else {
            return Ok(false);
        };
        if parse_guard_compare(split, BinaryOperator::Less) != Some(0) {
            return Ok(false);
        }
        let [Statement::If { condition: ladder2, then_body: mid, else_body: arm3 }] = high_arm.as_slice()
        else {
            return Ok(false);
        };
        let Some(k2) = parse_guard_compare(ladder2, BinaryOperator::Greater) else {
            return Ok(false);
        };
        // The INTEGRAL block: [*iptr = x[*one], *(int*)&x &= SIGN,
        // *(1+(int*)&x) = 0, Return(x)] — the x*one fold makes the first
        // store a plain stfd (measured: no fmul).
        let is_integral = |body: &[Statement]| -> bool {
            let [Statement::Store { target: tp, value: vp }, Statement::Store { target: t0, value: v0 }, Statement::Store { target: t1, value: v1 }, Statement::Return(Some(Expression::Variable(rx)))] =
                body
            else {
                return false;
            };
            let pointer_store_ok =
                matches!(tp, Expression::Dereference { pointer }
                    if matches!(pointer.as_ref(), Expression::Variable(v) if v == iptr));
            let value_is_x = matches!(vp, Expression::Variable(v) if v == x)
                || matches!(vp, Expression::Binary { operator: BinaryOperator::Multiply, left, right }
                    if matches!(left.as_ref(), Expression::Variable(v) if v == x)
                        && matches!(right.as_ref(), Expression::FloatLiteral(one) if *one == 1.0));
            rx == x
                && pointer_store_ok
                && value_is_x
                && crate::frame::pun_word_offset_pub(t0, x) == Some(0)
                && crate::frame::pun_word_offset_pub(t1, x) == Some(4)
                && crate::analysis::constant_value(v1) == Some(0)
                && matches!(v0, Expression::Binary { operator: BinaryOperator::BitAnd, left, right }
                    if crate::analysis::constant_value(right).map(|c| c as u32) == Some(0x8000_0000)
                        && (crate::frame::pun_word_offset_pub(left, x) == Some(0)
                            || matches!(left.as_ref(), Expression::Variable(v) if local_index(v) == Some(0))))
        };
        // A pointer-store pair + fraction return:
        //   [*(int*)iptr = HIGH, *(1+(int*)iptr) = LOW, Return(x - *iptr)]
        // HIGH: i0 & SIGN (arm1) / i0 & ~i (arm2) / i0 (arm3);
        // LOW: 0 (arm1/arm2) / i1 & ~i (arm3); arm1 returns plain x.
        enum HighForm {
            SignOnly,
            AndcShift,
            Plain,
        }
        enum LowForm {
            Zero,
            AndcShift,
        }
        let parse_pointer_arm = |body: &[Statement], fraction: bool| -> Option<(HighForm, LowForm)> {
            let [Statement::Store { target: t0, value: v0 }, Statement::Store { target: t1, value: v1 }, Statement::Return(Some(ret))] =
                body
            else {
                return None;
            };
            if pointer_word_offset(t0, iptr) != Some(0) || pointer_word_offset(t1, iptr) != Some(4) {
                return None;
            }
            if fraction {
                let ok = matches!(ret, Expression::Binary { operator: BinaryOperator::Subtract, left, right }
                    if matches!(left.as_ref(), Expression::Variable(v) if v == x)
                        && matches!(right.as_ref(), Expression::Dereference { pointer }
                            if matches!(pointer.as_ref(), Expression::Variable(v) if v == iptr)));
                if !ok {
                    return None;
                }
            } else if !matches!(ret, Expression::Variable(v) if v == x) {
                return None;
            }
            let high = if matches!(v0, Expression::Variable(v) if local_index(v) == Some(0)) {
                HighForm::Plain
            } else if let Expression::Binary { operator: BinaryOperator::BitAnd, left, right } = v0 {
                if !matches!(left.as_ref(), Expression::Variable(v) if local_index(v) == Some(0)) {
                    return None;
                }
                match right.as_ref() {
                    Expression::Unary { operator: UnaryOperator::BitNot, operand }
                        if matches!(operand.as_ref(), Expression::Variable(v) if v == shift) =>
                    {
                        HighForm::AndcShift
                    }
                    other if crate::analysis::constant_value(other).map(|c| c as u32)
                        == Some(0x8000_0000) =>
                    {
                        HighForm::SignOnly
                    }
                    _ => return None,
                }
            } else {
                return None;
            };
            let low = if crate::analysis::constant_value(v1) == Some(0) {
                LowForm::Zero
            } else if matches!(v1, Expression::Binary { operator: BinaryOperator::BitAnd, left, right }
                if matches!(left.as_ref(), Expression::Variable(v) if local_index(v) == Some(1))
                    && matches!(right.as_ref(), Expression::Unary { operator: UnaryOperator::BitNot, operand }
                        if matches!(operand.as_ref(), Expression::Variable(v) if v == shift)))
            {
                LowForm::AndcShift
            } else {
                return None;
            };
            Some((high, low))
        };
        // arm1: the sign-only pointer pair, plain return.
        if !matches!(parse_pointer_arm(arm1, false), Some((HighForm::SignOnly, LowForm::Zero))) {
            return Ok(false);
        }
        // arm2: [i = C >> j0, If{((i0&i)|i1)==0, integral, pointer-frac}].
        let [Statement::Assign { name: a2_shift, value: a2_value }, Statement::If { condition: a2_test, then_body: a2_int, else_body: a2_frac }] =
            arm2.as_slice()
        else {
            return Ok(false);
        };
        if a2_shift != shift {
            return Ok(false);
        }
        let Some((a2_mask, a2_logical, a2_off)) = parse_shift_init(a2_value, guard.name) else {
            return Ok(false);
        };
        if a2_logical || a2_off != 0 || i16::try_from(a2_mask).is_ok() {
            return Ok(false);
        }
        let a2_test_ok = matches!(a2_test, Expression::Binary { operator: BinaryOperator::Equal, left, right }
            if crate::analysis::constant_value(right) == Some(0)
                && matches!(left.as_ref(), Expression::Binary { operator: BinaryOperator::BitOr, left: ol, right: or }
                    if matches!(or.as_ref(), Expression::Variable(v) if local_index(v) == Some(1))
                        && matches!(ol.as_ref(), Expression::Binary { operator: BinaryOperator::BitAnd, left: al, right: ar }
                            if matches!(al.as_ref(), Expression::Variable(v) if local_index(v) == Some(0))
                                && matches!(ar.as_ref(), Expression::Variable(v) if v == shift))));
        if !a2_test_ok
            || !is_integral(a2_int)
            || !matches!(parse_pointer_arm(a2_frac, true), Some((HighForm::AndcShift, LowForm::Zero)))
        {
            return Ok(false);
        }
        // mid: the integral block.
        if !is_integral(mid) {
            return Ok(false);
        }
        // arm3: [i = (unsigned)C >> (j0-K), If{(i1&i)==0, integral, pointer-frac}].
        let [Statement::Assign { name: a3_shift, value: a3_value }, Statement::If { condition: a3_test, then_body: a3_int, else_body: a3_frac }] =
            arm3.as_slice()
        else {
            return Ok(false);
        };
        if a3_shift != shift {
            return Ok(false);
        }
        let Some((a3_mask, a3_logical, a3_off)) = parse_shift_init(a3_value, guard.name) else {
            return Ok(false);
        };
        let a3_mask = a3_mask as u32 as i32 as i64;
        let (Ok(a3_mask_small), Ok(a3_off_neg)) = (i16::try_from(a3_mask), i16::try_from(-a3_off))
        else {
            return Ok(false);
        };
        if !a3_logical || a3_off == 0 {
            return Ok(false);
        }
        let a3_test_ok = matches!(a3_test, Expression::Binary { operator: BinaryOperator::Equal, left, right }
            if crate::analysis::constant_value(right) == Some(0)
                && matches!(left.as_ref(), Expression::Binary { operator: BinaryOperator::BitAnd, left: al, right: ar }
                    if matches!(al.as_ref(), Expression::Variable(v) if local_index(v) == Some(1))
                        && matches!(ar.as_ref(), Expression::Variable(v) if v == shift)));
        if !a3_test_ok
            || !is_integral(a3_int)
            || !matches!(parse_pointer_arm(a3_frac, true), Some((HighForm::Plain, LowForm::AndcShift)))
        {
            return Ok(false);
        }
        // -- emit (registers per the capture; r3 = the live pointer param) --
        let (i0_reg, i1_reg, j0_reg, temp) = (5u8, 6u8, 7u8, 4u8);
        self.frame_size = 16;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: i0_reg, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: i1_reg, a: 1, offset: 12 });
        match guard.mask {
            Some(mask) => {
                let rotated = ((32 - guard.shift as u32) % 32) as u8;
                let Some((begin, end)) = crate::analysis::rlwinm_mask(mask) else {
                    return Err(Diagnostic::error("guard mask is not a run (roadmap)"));
                };
                self.output.instructions.push(Instruction::RotateAndMask {
                    a: temp,
                    s: i0_reg,
                    shift: rotated,
                    begin,
                    end,
                });
            }
            None => {
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate {
                    a: temp,
                    s: i0_reg,
                    shift: guard.shift,
                });
            }
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: j0_reg,
            a: temp,
            immediate: i16::try_from(-guard.offset_k).expect("validated"),
        });
        let epilogue = self.fresh_label();
        let ladder2_at = self.fresh_label();
        let arm2_at = self.fresh_label();
        let arm3_at = self.fresh_label();
        // The integral block: `*iptr = x` + the sign-only pun store + the
        // f1 reload — the stfd through the pointer schedules AFTER the pun
        // stores (measured), and the scratch is the temp r4 (the pointer
        // stays live here).
        let integral = |generator: &mut Self| {
            generator.output.instructions.push(Instruction::RotateAndMask {
                a: temp,
                s: i0_reg,
                shift: 0,
                begin: 0,
                end: 0,
            });
            generator.output.instructions.push(Instruction::load_immediate(0, 0));
            generator.output.instructions.push(Instruction::StoreWord { s: temp, a: 1, offset: 8 });
            generator.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 12 });
            generator.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 3, offset: 0 });
            generator.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 8 });
        };
        let fraction = |generator: &mut Self| {
            generator.output.instructions.push(Instruction::LoadFloatDouble { d: 0, a: 3, offset: 0 });
            generator.output.instructions.push(Instruction::FloatSubtractDouble { d: 1, a: 1, b: 0 });
        };
        self.output.instructions.push(Instruction::CompareWordImmediate { a: j0_reg, immediate: k1 });
        self.emit_branch_conditional_to(4, 0, ladder2_at); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: j0_reg, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, arm2_at); // bge
        // arm1: the sign pair through the pointer.
        self.output.instructions.push(Instruction::RotateAndMask { a: temp, s: i0_reg, shift: 0, begin: 0, end: 0 });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord { s: temp, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 3, offset: 4 });
        self.emit_branch_to(epilogue);
        // arm2.
        self.bind_label(arm2_at);
        let a2_lis = ((a2_mask + 0x8000) >> 16) << 16;
        self.output.instructions.push(Instruction::load_immediate_shifted(temp, (a2_lis >> 16) as i16));
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: temp, immediate: a2_mask as i16 });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicWord { a: temp, s: 0, b: j0_reg });
        self.output.instructions.push(Instruction::And { a: 0, s: i0_reg, b: temp });
        self.output.instructions.push(Instruction::OrRecord { a: 0, s: i1_reg, b: 0 });
        let a2_frac_at = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, a2_frac_at); // bne
        integral(self);
        self.emit_branch_to(epilogue);
        self.bind_label(a2_frac_at);
        self.output.instructions.push(Instruction::AndComplement { a: temp, s: i0_reg, b: temp });
        self.output.instructions.push(Instruction::load_immediate(0, 0));
        self.output.instructions.push(Instruction::StoreWord { s: temp, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 3, offset: 4 });
        fraction(self);
        self.emit_branch_to(epilogue);
        // ladder 2 + mid.
        self.bind_label(ladder2_at);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: j0_reg, immediate: k2 });
        self.emit_branch_conditional_to(4, 1, arm3_at); // ble
        integral(self);
        self.emit_branch_to(epilogue);
        // arm3.
        self.bind_label(arm3_at);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: j0_reg, immediate: a3_off_neg });
        self.output.instructions.push(Instruction::load_immediate(temp, a3_mask_small));
        self.output.instructions.push(Instruction::ShiftRightWord { a: temp, s: temp, b: 0 });
        self.output.instructions.push(Instruction::AndRecord { a: 0, s: i1_reg, b: temp });
        let a3_frac_at = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, a3_frac_at); // bne
        integral(self);
        self.emit_branch_to(epilogue);
        self.bind_label(a3_frac_at);
        self.output.instructions.push(Instruction::StoreWord { s: i0_reg, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::AndComplement { a: 0, s: i1_reg, b: temp });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 3, offset: 4 });
        fraction(self);
        self.bind_label(epilogue);
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // Pre-pool labels (measured @24 on the s_modf object vs the +0
        // base's @5).
        self.output.anonymous_label_bump += 19;
        Ok(true)
    }

    /// THE COMPOSER (fire 403): the full three-arm s_floor ladder —
    /// `if (j0<K1) { if (j0<0) ARM1 else ARM2 } else if (j0>K2) MID else
    /// ARM3` + writebacks. Arms are the standalone byte-exact templates
    /// with in-arm constants; registers come from int_alloc v3 with j0 as
    /// the SCRUTINEE (assigned last — r7 in the capture) and the arm
    /// shifts as ARM-DEFINED (they join the death-asc pool at r4). One
    /// JOIN (the stores) and one EPI serve every arm.
    fn try_punned_ladder_writeback(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        if function.return_type != Type::Double
            || !function.guards.is_empty()
            || function_makes_call(function)
            || self.non_leaf
        {
            return Ok(false);
        }
        let Some(Expression::Variable(returned)) = &function.return_expression else {
            return Ok(false);
        };
        let Some(first) = function.parameters.first() else {
            return Ok(false);
        };
        if first.parameter_type != Type::Double || returned != &first.name {
            return Ok(false);
        }
        let x = first.name.as_str();
        // Locals: initialized punned pair + guard; uninitialized unsigned
        // shift + carry (the normalizer folds only the leading assigns).
        let mut locals: Vec<(&str, i16)> = Vec::new();
        let mut guard: Option<GuardLocal> = None;
        let mut shift: Option<&str> = None;
        let mut carry: Option<&str> = None;
        for local in &function.locals {
            if local.array_length.is_some() {
                return Ok(false);
            }
            match (&local.initializer, local.declared_type) {
                (Some(init), Type::Int) => {
                    if let Some(offset) = crate::frame::pun_word_offset_pub(init, x) {
                        if locals.iter().any(|&(_, seen)| seen == offset) {
                            return Ok(false);
                        }
                        locals.push((local.name.as_str(), offset));
                    } else if guard.is_none() {
                        let Some(parsed) = parse_guard_init(local.name.as_str(), init) else {
                            return Ok(false);
                        };
                        guard = Some(parsed);
                    } else {
                        return Ok(false);
                    }
                }
                (None, Type::UnsignedInt) if shift.is_none() => shift = Some(local.name.as_str()),
                (None, Type::UnsignedInt) if carry.is_none() => carry = Some(local.name.as_str()),
                _ => return Ok(false),
            }
        }
        let (Some(guard), Some(shift), Some(carry)) = (guard, shift, carry) else {
            return Ok(false);
        };
        if locals.len() != 2
            || !locals.iter().any(|&(name, _)| name == guard.source)
            || guard.offset_k == 0
            || i16::try_from(-guard.offset_k).is_err()
        {
            return Ok(false);
        }
        let local_index = |name: &str| locals.iter().position(|&(local, _)| local == name);
        let i0 = local_index(guard.source).expect("checked");
        let i1 = 1 - i0;
        // The outer ladder + stores.
        let [Statement::If { condition: ladder1, then_body: low_arm, else_body: high_arm }, store_statements @ ..] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        if store_statements.len() != 2 {
            return Ok(false);
        }
        for (statement, &(name, offset)) in store_statements.iter().zip(&locals) {
            let Statement::Store { target, value } = statement else {
                return Ok(false);
            };
            if crate::frame::pun_word_offset_pub(target, x) != Some(offset)
                || !matches!(value, Expression::Variable(read) if read == name)
            {
                return Ok(false);
            }
        }
        // ladder1: j0 < K1.
        let parse_guard_compare = |condition: &Expression, operator: BinaryOperator| -> Option<i16> {
            let Expression::Binary { operator: op, left, right } = condition else { return None };
            if *op != operator || !matches!(left.as_ref(), Expression::Variable(v) if v == guard.name) {
                return None;
            }
            crate::analysis::constant_value(right).and_then(|k| i16::try_from(k).ok())
        };
        let Some(k1) = parse_guard_compare(ladder1, BinaryOperator::Less) else {
            return Ok(false);
        };
        // low_arm = [If{j0<0, arm1, arm2}].
        let [Statement::If { condition: split, then_body: arm1, else_body: arm2 }] = low_arm.as_slice()
        else {
            return Ok(false);
        };
        if parse_guard_compare(split, BinaryOperator::Less) != Some(0) {
            return Ok(false);
        }
        // high_arm = [If{j0>K2, mid, arm3}].
        let [Statement::If { condition: ladder2, then_body: mid, else_body: arm3 }] = high_arm.as_slice()
        else {
            return Ok(false);
        };
        let Some(k2) = parse_guard_compare(ladder2, BinaryOperator::Greater) else {
            return Ok(false);
        };
        // mid = [If{j0==K3, [Return x+x], [Return x]}].
        let [Statement::If { condition: mid_cond, then_body: mid_then, else_body: mid_else }] =
            mid.as_slice()
        else {
            return Ok(false);
        };
        let Some(k3) = parse_guard_compare(mid_cond, BinaryOperator::Equal) else {
            return Ok(false);
        };
        let mid_ok = matches!(mid_then.as_slice(),
                [Statement::Return(Some(Expression::Binary { operator: BinaryOperator::Add, left, right }))]
                    if matches!((left.as_ref(), right.as_ref()),
                        (Expression::Variable(a), Expression::Variable(b)) if a == x && b == x))
            && matches!(mid_else.as_slice(),
                [Statement::Return(Some(Expression::Variable(v)))] if v == x);
        if !mid_ok {
            return Ok(false);
        }
        // ARM1 (G3): If{huge+x>0, [If{i0>=0, [i0=i1=0], [If{((i0&M)|i1)!=0, [i0=HIGH, i1=0]}]}]}.
        let [Statement::If { condition: guard1_cond, then_body: guard1_body, else_body: guard1_else }] =
            arm1.as_slice()
        else {
            return Ok(false);
        };
        let Some((huge_bits, zero_bits)) = float_guard_condition(guard1_cond) else {
            return Ok(false);
        };
        if !guard1_else.is_empty() {
            return Ok(false);
        }
        let [Statement::If { condition: sign1, then_body: sign1_then, else_body: sign1_else }] =
            guard1_body.as_slice()
        else {
            return Ok(false);
        };
        // The sign comparison: `i0 >= 0` (s_floor) or `i0 < 0` (s_ceil) —
        // the emitted branch is the inverted sense to the else arm either
        // way.
        let Expression::Binary { operator: sign1_op, left: sign1_l, right: sign1_r } = sign1 else {
            return Ok(false);
        };
        let sign1_branch = match sign1_op {
            BinaryOperator::GreaterEqual => (12u8, 0u8), // blt
            BinaryOperator::Less => (4u8, 0u8),          // bge
            _ => return Ok(false),
        };
        if !matches!(sign1_l.as_ref(), Expression::Variable(v) if local_index(v) == Some(i0))
            || crate::analysis::constant_value(sign1_r) != Some(0)
        {
            return Ok(false);
        }
        // A constant pair `[i0 = C, i1 = C']` (each li or lis form), or the
        // chained `i0 = i1 = 0` (emitted inner-first).
        enum ConstPair {
            Chained0,
            Pair { first: i64, second: i64 },
        }
        let parse_pair = |body: &[Statement]| -> Option<ConstPair> {
            match body {
                [Statement::Assign { name, value: Expression::Assign { target, value } }]
                    if local_index(name) == Some(i0)
                        && matches!(target.as_ref(), Expression::Variable(v) if local_index(v) == Some(i1))
                        && crate::analysis::constant_value(value) == Some(0) =>
                {
                    Some(ConstPair::Chained0)
                }
                [Statement::Assign { name: a, value: av }, Statement::Assign { name: b, value: bv }]
                    if local_index(a) == Some(i0) && local_index(b) == Some(i1) =>
                {
                    let first = crate::analysis::constant_value(av)? as u32 as i32 as i64;
                    let second = crate::analysis::constant_value(bv)? as u32 as i32 as i64;
                    let representable = |constant: i64| {
                        i16::try_from(constant).is_ok() || constant & 0xffff == 0
                    };
                    (representable(first) && representable(second))
                        .then_some(ConstPair::Pair { first, second })
                }
                _ => None,
            }
        };
        let Some(sign1_pair) = parse_pair(sign1_then) else {
            return Ok(false);
        };
        // else: If{((i0 [& M]) | i1) != 0, [pair]} — the mask is optional
        // (s_ceil's plain `(i0 | i1) != 0`).
        let [Statement::If { condition: mag_cond, then_body: mag_then, else_body: mag_else }] =
            sign1_else.as_slice()
        else {
            return Ok(false);
        };
        if !mag_else.is_empty() {
            return Ok(false);
        }
        let Some(mag_mask) = (|| {
            let Expression::Binary { operator: BinaryOperator::NotEqual, left, right } = mag_cond
            else {
                return None;
            };
            if crate::analysis::constant_value(right) != Some(0) {
                return None;
            }
            let Expression::Binary { operator: BinaryOperator::BitOr, left: or_l, right: or_r } =
                left.as_ref()
            else {
                return None;
            };
            if !matches!(or_r.as_ref(), Expression::Variable(v) if local_index(v) == Some(i1)) {
                return None;
            }
            match or_l.as_ref() {
                Expression::Variable(v) if local_index(v) == Some(i0) => Some(None),
                Expression::Binary { operator: BinaryOperator::BitAnd, left: and_l, right: and_r }
                    if matches!(and_l.as_ref(), Expression::Variable(v) if local_index(v) == Some(i0)) =>
                {
                    let mask = crate::analysis::constant_value(and_r)?;
                    let (begin, end) = crate::analysis::rlwinm_mask(mask)?;
                    Some(Some((begin, end)))
                }
                _ => None,
            }
        })() else {
            return Ok(false);
        };
        let Some(ConstPair::Pair { first: mag_first, second: mag_second }) = parse_pair(mag_then)
        else {
            return Ok(false);
        };
        // ARM2 (fire 399): [i = C >> j0, If{test, [Ret x]}, If{huge, [If{i0<0, [i0 += C2>>j0]}, i0 &= ~i, i1 = 0]}].
        let [Statement::Assign { name: a2_shift_name, value: a2_shift_value }, Statement::If { condition: a2_test, then_body: a2_ret, else_body: a2_test_else }, Statement::If { condition: a2_guard, then_body: a2_guard_body, else_body: a2_guard_else }] =
            arm2.as_slice()
        else {
            return Ok(false);
        };
        if a2_shift_name != shift || !a2_test_else.is_empty() || !a2_guard_else.is_empty() {
            return Ok(false);
        }
        let Some((a2_mask, a2_logical, a2_offset)) = parse_shift_init(a2_shift_value, guard.name)
        else {
            return Ok(false);
        };
        if a2_logical || a2_offset != 0 || i16::try_from(a2_mask).is_ok() {
            return Ok(false);
        }
        let a2_lis = ((a2_mask + 0x8000) >> 16) << 16;
        if !matches!(a2_ret.as_slice(), [Statement::Return(Some(Expression::Variable(v)))] if v == x) {
            return Ok(false);
        }
        // test: ((i0 & i) | i1) == 0
        let a2_test_ok = (|| {
            let Expression::Binary { operator: BinaryOperator::Equal, left, right } = a2_test else {
                return false;
            };
            if crate::analysis::constant_value(right) != Some(0) {
                return false;
            }
            let Expression::Binary { operator: BinaryOperator::BitOr, left: or_l, right: or_r } =
                left.as_ref()
            else {
                return false;
            };
            matches!(or_r.as_ref(), Expression::Variable(v) if local_index(v) == Some(i1))
                && matches!(or_l.as_ref(),
                    Expression::Binary { operator: BinaryOperator::BitAnd, left: al, right: ar }
                        if matches!(al.as_ref(), Expression::Variable(v) if local_index(v) == Some(i0))
                            && matches!(ar.as_ref(), Expression::Variable(v) if v == shift))
        })();
        if !a2_test_ok || float_guard_condition(a2_guard) != Some((huge_bits, zero_bits)) {
            return Ok(false);
        }
        let [Statement::If { condition: a2_sign, then_body: a2_add, else_body: a2_sign_else }, Statement::Assign { name: a2_andc_name, value: a2_andc_value }, Statement::Assign { name: a2_rw_name, value: a2_rw_value }] =
            a2_guard_body.as_slice()
        else {
            return Ok(false);
        };
        let parse_sign = |condition: &Expression| -> Option<(u8, u8)> {
            let Expression::Binary { operator, left, right } = condition else { return None };
            if !matches!(left.as_ref(), Expression::Variable(v) if local_index(v) == Some(i0))
                || crate::analysis::constant_value(right) != Some(0)
            {
                return None;
            }
            match operator {
                BinaryOperator::Less => Some((4, 0)),    // bge — skip when >= 0
                BinaryOperator::Greater => Some((4, 1)), // ble — skip when <= 0
                _ => None,
            }
        };
        let Some(a2_sign_branch) = parse_sign(a2_sign) else {
            return Ok(false);
        };
        let a2_ok = a2_sign_else.is_empty()
            && matches!(a2_add.as_slice(), [Statement::Assign { name, value }]
                if local_index(name) == Some(i0)
                    && matches!(value, Expression::Binary { operator: BinaryOperator::Add, left, right }
                        if matches!(left.as_ref(), Expression::Variable(v) if local_index(v) == Some(i0))
                            && matches!(right.as_ref(),
                                Expression::Binary { operator: BinaryOperator::ShiftRight, left: c2, right: by }
                                    if crate::analysis::constant_value(c2) == Some(a2_lis)
                                        && matches!(by.as_ref(), Expression::Variable(v) if v == guard.name))))
            && local_index(a2_andc_name) == Some(i0)
            && matches!(a2_andc_value, Expression::Binary { operator: BinaryOperator::BitAnd, left, right }
                if matches!(left.as_ref(), Expression::Variable(v) if local_index(v) == Some(i0))
                    && matches!(right.as_ref(), Expression::Unary { operator: UnaryOperator::BitNot, operand }
                        if matches!(operand.as_ref(), Expression::Variable(v) if v == shift)))
            && local_index(a2_rw_name) == Some(i1)
            && crate::analysis::constant_value(a2_rw_value) == Some(0);
        if !a2_ok {
            return Ok(false);
        }
        // ARM3 (fire 400): [i = (unsigned)C >> (j0-K4), If{(i1&i)==0, [Ret x]},
        //   If{huge, [If{i0<0, [If{j0==K5, [i0+=1], [carry]}]}, i1 &= ~i]}].
        let [Statement::Assign { name: a3_shift_name, value: a3_shift_value }, Statement::If { condition: a3_test, then_body: a3_ret, else_body: a3_test_else }, Statement::If { condition: a3_guard, then_body: a3_guard_body, else_body: a3_guard_else }] =
            arm3.as_slice()
        else {
            return Ok(false);
        };
        if a3_shift_name != shift || !a3_test_else.is_empty() || !a3_guard_else.is_empty() {
            return Ok(false);
        }
        let Some((a3_mask, a3_logical, a3_offset)) = parse_shift_init(a3_shift_value, guard.name)
        else {
            return Ok(false);
        };
        let a3_mask = a3_mask as u32 as i32 as i64;
        let (Ok(a3_mask_small), Ok(a3_offset_neg)) = (i16::try_from(a3_mask), i16::try_from(-a3_offset))
        else {
            return Ok(false);
        };
        if !a3_logical || a3_offset == 0 {
            return Ok(false);
        }
        if !matches!(a3_ret.as_slice(), [Statement::Return(Some(Expression::Variable(v)))] if v == x) {
            return Ok(false);
        }
        let a3_test_ok = matches!(a3_test, Expression::Binary { operator: BinaryOperator::Equal, left, right }
            if crate::analysis::constant_value(right) == Some(0)
                && matches!(left.as_ref(), Expression::Binary { operator: BinaryOperator::BitAnd, left: al, right: ar }
                    if matches!(al.as_ref(), Expression::Variable(v) if local_index(v) == Some(i1))
                        && matches!(ar.as_ref(), Expression::Variable(v) if v == shift)));
        if !a3_test_ok || float_guard_condition(a3_guard) != Some((huge_bits, zero_bits)) {
            return Ok(false);
        }
        let [Statement::If { condition: a3_sign, then_body: a3_diamond, else_body: a3_sign_else }, Statement::Assign { name: a3_andc_name, value: a3_andc_value }] =
            a3_guard_body.as_slice()
        else {
            return Ok(false);
        };
        let Some(a3_sign_branch) = parse_sign(a3_sign) else {
            return Ok(false);
        };
        let a3_frame_ok = a3_sign_else.is_empty()
            && local_index(a3_andc_name) == Some(i1)
            && matches!(a3_andc_value, Expression::Binary { operator: BinaryOperator::BitAnd, left, right }
                if matches!(left.as_ref(), Expression::Variable(v) if local_index(v) == Some(i1))
                    && matches!(right.as_ref(), Expression::Unary { operator: UnaryOperator::BitNot, operand }
                        if matches!(operand.as_ref(), Expression::Variable(v) if v == shift)));
        if !a3_frame_ok {
            return Ok(false);
        }
        let [Statement::If { condition: eq_cond, then_body: eq_then, else_body: eq_else }] =
            a3_diamond.as_slice()
        else {
            return Ok(false);
        };
        let Some(k5) = parse_guard_compare(eq_cond, BinaryOperator::Equal) else {
            return Ok(false);
        };
        let inc_ok = |body: &[Statement]| {
            matches!(body, [Statement::Assign { name, value }]
                if local_index(name) == Some(i0)
                    && matches!(value, Expression::Binary { operator: BinaryOperator::Add, left, right }
                        if matches!(left.as_ref(), Expression::Variable(v) if local_index(v) == Some(i0))
                            && crate::analysis::constant_value(right) == Some(1)))
        };
        if !inc_ok(eq_then) {
            return Ok(false);
        }
        let [Statement::Assign { name: j_name, value: j_value }, Statement::If { condition: carry_cond, then_body: carry_then, else_body: carry_else }, Statement::Assign { name: copy_name, value: copy_value }] =
            eq_else.as_slice()
        else {
            return Ok(false);
        };
        let Some(k6) = (|| {
            if j_name != carry {
                return None;
            }
            let Expression::Binary { operator: BinaryOperator::Add, left: base, right: one_shift } =
                j_value
            else {
                return None;
            };
            if !matches!(base.as_ref(), Expression::Variable(v) if local_index(v) == Some(i1)) {
                return None;
            }
            let Expression::Binary { operator: BinaryOperator::ShiftLeft, left: one, right: amount } =
                one_shift.as_ref()
            else {
                return None;
            };
            if crate::analysis::constant_value(one) != Some(1) {
                return None;
            }
            let Expression::Binary { operator: BinaryOperator::Subtract, left: k6, right: by } =
                amount.as_ref()
            else {
                return None;
            };
            if !matches!(by.as_ref(), Expression::Variable(v) if v == guard.name) {
                return None;
            }
            crate::analysis::constant_value(k6).and_then(|k| i16::try_from(k).ok())
        })() else {
            return Ok(false);
        };
        let carry_ok = carry_else.is_empty()
            && matches!(carry_cond, Expression::Binary { operator: BinaryOperator::Less, left, right }
                if matches!(left.as_ref(), Expression::Variable(v) if v == carry)
                    && matches!(right.as_ref(), Expression::Variable(v) if local_index(v) == Some(i1)))
            && inc_ok(carry_then)
            && local_index(copy_name) == Some(i1)
            && matches!(copy_value, Expression::Variable(v) if v == carry);
        if !carry_ok {
            return Ok(false);
        }
        // -- the model (positions computed from the emission template) --
        use mwcc_vreg::int_alloc::{allocate, Class, Value};
        // arm1's sign diamond: [cmpwi, branch, then(1 or 2), b] + else
        // ([clrlwi]?, or., beq, lis, li, b).
        let sign1_then_len: u32 = match &sign1_pair {
            ConstPair::Chained0 => 2,
            ConstPair::Pair { .. } => 2,
        };
        let mag_len: u32 = if mag_mask.is_some() { 6 } else { 5 };
        let arm1_diamond = 2 + sign1_then_len + 1 + mag_len;
        let arm2_base = 15 + arm1_diamond; // preamble 0..9 + float(4)+ble @10..14
        let ladder2 = arm2_base + 19;
        let arm3_base = ladder2 + 6;
        let join_at = arm3_base + 26;
        let values = [
            Value { class: Class::Temp, def: 4, last: 5 },
            Value { class: Class::Temp, def: arm2_base, last: arm2_base + 14 }, // lis..sraw2 (CSE)
            Value { class: Class::Mask, def: arm3_base + 1, last: arm3_base + 2 },
            Value { class: Class::Mask, def: arm3_base + 18, last: arm3_base + 19 }, // the ONE
            Value { class: Class::Scrutinee, def: 5, last: arm3_base + 17 },   // ..subfic
            Value { class: Class::LoadSurviving, def: 2, last: join_at },
            Value { class: Class::LoadSurviving, def: 3, last: join_at + 1 },
            Value { class: Class::ArmShift, def: arm2_base + 2, last: arm2_base + 16 },
            Value { class: Class::ArmShift, def: arm3_base + 2, last: arm3_base + 25 },
        ];
        let registers = allocate(&values);
        let extract_temp = registers[0];
        let a2_temp = registers[1];
        let a3_mask_reg = registers[2];
        let one_reg = registers[3];
        let j0_reg = registers[4];
        let i0_reg = if i0 == 0 { registers[5] } else { registers[6] };
        let i1_reg = if i0 == 0 { registers[6] } else { registers[5] };
        let a2_i = registers[7];
        let a3_i = registers[8];
        // NB: loads emit in frame-offset order = locals order; registers[5]
        // belongs to locals[0].
        let load0 = registers[5];
        let load1 = registers[6];
        // -- emit --
        self.frame_size = 16;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: load0, a: 1, offset: 8 + locals[0].1 });
        self.output.instructions.push(Instruction::LoadWord { d: load1, a: 1, offset: 8 + locals[1].1 });
        match guard.mask {
            Some(mask) => {
                let rotated = ((32 - guard.shift as u32) % 32) as u8;
                let Some((begin, end)) = crate::analysis::rlwinm_mask(mask) else {
                    return Err(Diagnostic::error("guard mask is not a run (roadmap)"));
                };
                self.output.instructions.push(Instruction::RotateAndMask {
                    a: extract_temp,
                    s: i0_reg,
                    shift: rotated,
                    begin,
                    end,
                });
            }
            None => {
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate {
                    a: extract_temp,
                    s: i0_reg,
                    shift: guard.shift,
                });
            }
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: j0_reg,
            a: extract_temp,
            immediate: i16::try_from(-guard.offset_k).expect("validated"),
        });
        let join = self.fresh_label();
        let epilogue = self.fresh_label();
        let ladder2_at = self.fresh_label();
        let arm2_at = self.fresh_label();
        let arm3_at = self.fresh_label();
        // The ladder.
        self.output.instructions.push(Instruction::CompareWordImmediate { a: j0_reg, immediate: k1 });
        self.emit_branch_conditional_to(4, 0, ladder2_at); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: j0_reg, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, arm2_at); // bge
        // ARM1.
        self.load_double_constant(2, huge_bits);
        self.load_double_constant(0, zero_bits);
        self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 2, b: 1 });
        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
        self.emit_branch_conditional_to(4, 1, join); // ble
        let arm1_else = self.fresh_label();
        self.output.instructions.push(Instruction::CompareWordImmediate { a: i0_reg, immediate: 0 });
        self.emit_branch_conditional_to(sign1_branch.0, sign1_branch.1, arm1_else);
        let emit_constant = |generator: &mut Self, register: u8, constant: i64| {
            if let Ok(small) = i16::try_from(constant) {
                generator.output.instructions.push(Instruction::load_immediate(register, small));
            } else {
                generator
                    .output
                    .instructions
                    .push(Instruction::load_immediate_shifted(register, (constant >> 16) as i16));
            }
        };
        match &sign1_pair {
            ConstPair::Chained0 => {
                // The chained `i0 = i1 = 0` assigns inner-first.
                self.output.instructions.push(Instruction::load_immediate(i1_reg, 0));
                self.output.instructions.push(Instruction::load_immediate(i0_reg, 0));
            }
            ConstPair::Pair { first, second } => {
                emit_constant(self, i0_reg, *first);
                emit_constant(self, i1_reg, *second);
            }
        }
        self.emit_branch_to(join);
        self.bind_label(arm1_else);
        if let Some((begin, end)) = mag_mask {
            self.output.instructions.push(Instruction::RotateAndMask {
                a: 0,
                s: i0_reg,
                shift: 0,
                begin,
                end,
            });
            self.output.instructions.push(Instruction::OrRecord { a: 0, s: 0, b: i1_reg });
        } else {
            self.output.instructions.push(Instruction::OrRecord { a: 0, s: i0_reg, b: i1_reg });
        }
        self.emit_branch_conditional_to(12, 2, join); // beq
        emit_constant(self, i0_reg, mag_first);
        emit_constant(self, i1_reg, mag_second);
        self.emit_branch_to(join);
        // ARM2.
        self.bind_label(arm2_at);
        self.output.instructions.push(Instruction::load_immediate_shifted(a2_temp, (a2_lis >> 16) as i16));
        self.output.instructions.push(Instruction::AddImmediate {
            d: 0,
            a: a2_temp,
            immediate: a2_mask as i16,
        });
        self.output.instructions.push(Instruction::ShiftRightAlgebraicWord { a: a2_i, s: 0, b: j0_reg });
        self.output.instructions.push(Instruction::And { a: 0, s: i0_reg, b: a2_i });
        self.output.instructions.push(Instruction::OrRecord { a: 0, s: i1_reg, b: 0 });
        let a2_cont = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, a2_cont); // bne
        self.emit_branch_to(epilogue);
        self.bind_label(a2_cont);
        self.load_double_constant(2, huge_bits);
        self.load_double_constant(0, zero_bits);
        self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 2, b: 1 });
        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
        self.emit_branch_conditional_to(4, 1, join); // ble
        let a2_skip = self.fresh_label();
        self.output.instructions.push(Instruction::CompareWordImmediate { a: i0_reg, immediate: 0 });
        self.emit_branch_conditional_to(a2_sign_branch.0, a2_sign_branch.1, a2_skip);
        self.output.instructions.push(Instruction::ShiftRightAlgebraicWord { a: 0, s: a2_temp, b: j0_reg });
        self.output.instructions.push(Instruction::Add { d: i0_reg, a: i0_reg, b: 0 });
        self.bind_label(a2_skip);
        self.output.instructions.push(Instruction::AndComplement { a: i0_reg, s: i0_reg, b: a2_i });
        self.output.instructions.push(Instruction::load_immediate(i1_reg, 0));
        self.emit_branch_to(join);
        // LADDER 2 + MID.
        self.bind_label(ladder2_at);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: j0_reg, immediate: k2 });
        self.emit_branch_conditional_to(4, 1, arm3_at); // ble
        self.output.instructions.push(Instruction::CompareWordImmediate { a: j0_reg, immediate: k3 });
        self.emit_branch_conditional_to(4, 2, epilogue); // bne — return x
        self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 1, b: 1 });
        self.emit_branch_to(epilogue);
        // ARM3.
        self.bind_label(arm3_at);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: j0_reg, immediate: a3_offset_neg });
        self.output.instructions.push(Instruction::load_immediate(a3_mask_reg, a3_mask_small));
        self.output.instructions.push(Instruction::ShiftRightWord { a: a3_i, s: a3_mask_reg, b: 0 });
        self.output.instructions.push(Instruction::AndRecord { a: 0, s: i1_reg, b: a3_i });
        let a3_cont = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, a3_cont); // bne
        self.emit_branch_to(epilogue);
        self.bind_label(a3_cont);
        self.load_double_constant(2, huge_bits);
        self.load_double_constant(0, zero_bits);
        self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 2, b: 1 });
        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
        self.emit_branch_conditional_to(4, 1, join); // ble
        let a3_andc = self.fresh_label();
        let a3_carry = self.fresh_label();
        let a3_no_carry = self.fresh_label();
        self.output.instructions.push(Instruction::CompareWordImmediate { a: i0_reg, immediate: 0 });
        self.emit_branch_conditional_to(a3_sign_branch.0, a3_sign_branch.1, a3_andc);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: j0_reg, immediate: k5 });
        self.emit_branch_conditional_to(4, 2, a3_carry); // bne
        self.output.instructions.push(Instruction::AddImmediate { d: i0_reg, a: i0_reg, immediate: 1 });
        self.emit_branch_to(a3_andc);
        self.bind_label(a3_carry);
        self.output.instructions.push(Instruction::SubtractFromImmediate { d: 0, a: j0_reg, immediate: k6 });
        self.output.instructions.push(Instruction::load_immediate(one_reg, 1));
        self.output.instructions.push(Instruction::ShiftLeftWord { a: 0, s: one_reg, b: 0 });
        self.output.instructions.push(Instruction::Add { d: 0, a: i1_reg, b: 0 });
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: i1_reg });
        self.emit_branch_conditional_to(4, 0, a3_no_carry); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: i0_reg, a: i0_reg, immediate: 1 });
        self.bind_label(a3_no_carry);
        self.output.instructions.push(Instruction::move_register(i1_reg, 0));
        self.bind_label(a3_andc);
        self.output.instructions.push(Instruction::AndComplement { a: i1_reg, s: i1_reg, b: a3_i });
        // JOIN + EPI.
        self.bind_label(join);
        self.output.instructions.push(Instruction::StoreWord { s: load0, a: 1, offset: 8 + locals[0].1 });
        self.output.instructions.push(Instruction::StoreWord { s: load1, a: 1, offset: 8 + locals[1].1 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 8 });
        self.bind_label(epilogue);
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // Pre-pool labels (measured on the full s_floor object: real @45
        // vs the +0 base's @5).
        self.output.anonymous_label_bump += 40;
        Ok(true)
    }

    /// The SHIFT-WRITEBACK family (s_floor arm2's core): statements =
    /// `[i = C >> j0]  [if (test) return x]  [mutations...]  [stores...]`
    /// with a multi-use shifted mask. Registers come from the fitted
    /// int_alloc v2 model (13/13 captures — docs/int-allocator-frontier.md):
    /// a synthetic position pass numbers the template, values classify as
    /// Temp/Mask/Computed/Load{Discarded,Surviving}/Shift, and the model
    /// orders lowest-free assignment. Measured forms:
    ///   test: `((a & i) | b) == 0` (and + or., b FIRST) or `(a & i) == 0`
    ///     (and. record); skip = bne CONT; b EPI; CONT:.
    ///   mutations: `l &= ~i` (fused andc; TWO of them share one not r0),
    ///     `l &= K` (clrlwi r0, store from r0 — the home is read only),
    ///     `l = K` (li r0, store from r0 — the home is DISCARDED when it
    ///     was read in the test, and never loaded when read nowhere).
    fn try_punned_shift_writeback(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        if function.return_type != Type::Double
            || !function.guards.is_empty()
            || function_makes_call(function)
            || self.non_leaf
        {
            return Ok(false);
        }
        let Some(Expression::Variable(returned)) = &function.return_expression else {
            return Ok(false);
        };
        let Some(first) = function.parameters.first() else {
            return Ok(false);
        };
        if first.parameter_type != Type::Double || returned != &first.name {
            return Ok(false);
        }
        let x = first.name.as_str();
        // Roles come either from local INITIALIZERS (the normalizer folds
        // the leading assigns when nothing reassigns at top level — the
        // guarded-mutation forms) or from the LEADING assigns themselves
        // (top-level mutations make the normalizer refuse).
        let mut locals: Vec<(&str, i16)> = Vec::new();
        let mut guard: Option<GuardLocal> = None;
        let mut shift: Option<&str> = None;
        let mut mask_constant: Option<(i64, bool, i64)> = None; // (C, logical, amount offset)
        let mut cursor = 0usize;
        // The carry local (arm3's `j`) is assigned only inside the guard,
        // so the normalizer leaves it uninitialized while folding the rest.
        let mut carry_local: Option<&str> = None;
        let normalized = !function.locals.is_empty()
            && function.locals.iter().any(|local| local.initializer.is_some());
        if normalized {
            for local in &function.locals {
                if local.array_length.is_some() {
                    return Ok(false);
                }
                let Some(init) = local.initializer.as_ref() else {
                    if local.declared_type == Type::UnsignedInt && carry_local.is_none() {
                        carry_local = Some(local.name.as_str());
                        continue;
                    }
                    return Ok(false);
                };
                if local.declared_type == Type::UnsignedInt {
                    if shift.is_some() {
                        return Ok(false);
                    }
                    let Some(parsed) = &guard else { return Ok(false) };
                    let Some((constant, logical, offset)) = parse_shift_init(init, parsed.name)
                    else {
                        return Ok(false);
                    };
                    mask_constant = Some((constant, logical, offset));
                    shift = Some(local.name.as_str());
                    continue;
                }
                if local.declared_type != Type::Int {
                    return Ok(false);
                }
                if let Some(offset) = crate::frame::pun_word_offset_pub(init, x) {
                    if locals.iter().any(|&(_, seen)| seen == offset) {
                        return Ok(false);
                    }
                    locals.push((local.name.as_str(), offset));
                    continue;
                }
                if guard.is_some() {
                    return Ok(false);
                }
                let Some(parsed) = parse_guard_init(local.name.as_str(), init) else {
                    return Ok(false);
                };
                if !locals.iter().any(|&(name, _)| name == parsed.source) {
                    return Ok(false);
                }
                guard = Some(parsed);
            }
        } else {
            while let Some(Statement::Assign { name, value }) = function.statements.get(cursor) {
                let Some(declaration) = function.locals.iter().find(|local| &local.name == name) else {
                    return Ok(false);
                };
                if declaration.initializer.is_some() || declaration.array_length.is_some() {
                    return Ok(false);
                }
                if let Some(offset) = crate::frame::pun_word_offset_pub(value, x) {
                    if declaration.declared_type != Type::Int
                        || locals.iter().any(|&(_, seen)| seen == offset)
                    {
                        return Ok(false);
                    }
                    locals.push((name.as_str(), offset));
                    cursor += 1;
                    continue;
                }
                if guard.is_none() && declaration.declared_type == Type::Int {
                    if let Some(parsed) = parse_guard_init(name.as_str(), value) {
                        if locals.iter().any(|&(local, _)| local == parsed.source) {
                            guard = Some(parsed);
                            cursor += 1;
                            continue;
                        }
                    }
                    return Ok(false);
                }
                if shift.is_none() && declaration.declared_type == Type::UnsignedInt {
                    if let Some(parsed) = &guard {
                        if let Some((constant, logical, offset)) = parse_shift_init(value, parsed.name) {
                            mask_constant = Some((constant, logical, offset));
                            shift = Some(name.as_str());
                            cursor += 1;
                            continue;
                        }
                    }
                    return Ok(false);
                }
                return Ok(false);
            }
        }
        let (Some(guard), Some(shift), Some((mask_constant, logical_shift, amount_offset))) =
            (guard, shift, mask_constant)
        else {
            return Ok(false);
        };
        if i16::try_from(-amount_offset).is_err() {
            return Ok(false);
        }
        if locals.is_empty() || locals.len() > 2 {
            return Ok(false);
        }
        if guard.offset_k == 0 || i16::try_from(-guard.offset_k).is_err() {
            return Ok(false);
        }
        // j0 is consumed by the shift alone; the shift local is written once.
        let tail = &function.statements[cursor..];
        fn reads_in(statement: &Statement, name: &str) -> usize {
            match statement {
                Statement::Assign { value, .. } => count_name_occurrences(value, name),
                Statement::Store { target, value } => {
                    count_name_occurrences(target, name) + count_name_occurrences(value, name)
                }
                Statement::If { condition, then_body, else_body } => {
                    count_name_occurrences(condition, name)
                        + then_body.iter().map(|inner| reads_in(inner, name)).sum::<usize>()
                        + else_body.iter().map(|inner| reads_in(inner, name)).sum::<usize>()
                }
                Statement::Return(Some(value)) => count_name_occurrences(value, name),
                _ => 1,
            }
        }
        let guard_tail_reads: usize =
            tail.iter().map(|statement| reads_in(statement, guard.name)).sum();
        if guard_tail_reads > 2 {
            // Beyond the sign block's reads (the self-add's shift, or the
            // carry diamond's ==K4 + K3-j0 pair — validated structurally
            // below; unknown j0 uses fail the exact-form parses).
            return Ok(false);
        }
        // The early-return test.
        let Some(Statement::If { condition, then_body, else_body }) = tail.first() else {
            return Ok(false);
        };
        if !matches!(then_body.as_slice(), [Statement::Return(Some(Expression::Variable(v)))] if v == x)
            || !else_body.is_empty()
        {
            return Ok(false);
        }
        // `((a & i) | b) == 0` or `(a & i) == 0`, a/b punned, i the shift.
        let Expression::Binary { operator: BinaryOperator::Equal, left: test, right: zero } = condition
        else {
            return Ok(false);
        };
        if crate::analysis::constant_value(zero) != Some(0) {
            return Ok(false);
        }
        let local_index = |name: &str| locals.iter().position(|&(local, _)| local == name);
        let parse_and = |expr: &Expression| -> Option<usize> {
            let Expression::Binary { operator: BinaryOperator::BitAnd, left, right } = expr else {
                return None;
            };
            let Expression::Variable(a) = left.as_ref() else { return None };
            let Expression::Variable(i) = right.as_ref() else { return None };
            if i != shift {
                return None;
            }
            local_index(a)
        };
        let (test_and_local, test_or_local) = match test.as_ref() {
            Expression::Binary { operator: BinaryOperator::BitOr, left, right } => {
                let Some(a) = parse_and(left) else { return Ok(false) };
                let Expression::Variable(b) = right.as_ref() else { return Ok(false) };
                let Some(b) = local_index(b) else { return Ok(false) };
                (a, Some(b))
            }
            other => {
                let Some(a) = parse_and(other) else { return Ok(false) };
                (a, None)
            }
        };
        // An optional inexact guard wraps the mutations: `if (huge+x>0.0)
        // { [if (l<0) l += C2>>j0;] mutations }` (s_floor arm2). Inside
        // it a rewrite is CONDITIONAL — the original must survive the
        // guard-false path, so it lands in the home, not r0.
        let mut float_guard: Option<(u64, u64)> = None;
        enum SignBlock {
            Add { local: usize, constant: i64 },
            CarryDiamond {
                local: usize,          // i0 — takes +1
                other: usize,          // i1 — the carry source, receives j
                equal_bound: i16,      // j0 == K4
                shift_base: i16,       // K3 in `1 << (K3 - j0)`
            },
        }
        let mut sign_block: Option<SignBlock> = None;
        let mut mutation_statements: &[Statement] = &tail[1..];
        if let Some(Statement::If { condition, then_body, else_body }) = tail.get(1) {
            let Some(guard_bits) = float_guard_condition(condition) else {
                return Ok(false);
            };
            if !else_body.is_empty() {
                return Ok(false);
            }
            float_guard = Some(guard_bits);
            let mut body: &[Statement] = then_body;
            if let Some(Statement::If { condition, then_body: sign_body, else_body }) = body.first() {
                // `if (l < 0) ...`
                let Expression::Binary { operator: BinaryOperator::Less, left, right } = condition
                else {
                    return Ok(false);
                };
                if crate::analysis::constant_value(right) != Some(0) {
                    return Ok(false);
                }
                let Expression::Variable(signed) = left.as_ref() else { return Ok(false) };
                let Some(sign_local) = local_index(signed) else { return Ok(false) };
                if !else_body.is_empty() {
                    return Ok(false);
                }
                match sign_body.as_slice() {
                    // arm2: `l += C2 >> j0;`
                    [Statement::Assign { name: add_name, value: add_value }] => {
                        if local_index(add_name) != Some(sign_local) {
                            return Ok(false);
                        }
                        let Expression::Binary { operator: BinaryOperator::Add, left: base, right: shifted } =
                            add_value
                        else {
                            return Ok(false);
                        };
                        if !matches!(base.as_ref(), Expression::Variable(v) if v == add_name.as_str()) {
                            return Ok(false);
                        }
                        let Expression::Binary { operator: BinaryOperator::ShiftRight, left: c2, right: by } =
                            shifted.as_ref()
                        else {
                            return Ok(false);
                        };
                        let Some(c2) = crate::analysis::constant_value(c2) else { return Ok(false) };
                        if !matches!(by.as_ref(), Expression::Variable(v) if v == guard.name) {
                            return Ok(false);
                        }
                        sign_block = Some(SignBlock::Add { local: sign_local, constant: c2 });
                    }
                    // arm3: `if (j0 == K4) l += 1; else { j = other + (1 << (K3 - j0));
                    //        if (j < other) l += 1; other = j; }`
                    [Statement::If { condition, then_body, else_body }] => {
                        let Some(carry) = carry_local else { return Ok(false) };
                        let Expression::Binary { operator: BinaryOperator::Equal, left, right } = condition
                        else {
                            return Ok(false);
                        };
                        if !matches!(left.as_ref(), Expression::Variable(v) if v == guard.name) {
                            return Ok(false);
                        }
                        let Some(equal_bound) =
                            crate::analysis::constant_value(right).and_then(|k| i16::try_from(k).ok())
                        else {
                            return Ok(false);
                        };
                        // then: l += 1
                        let [Statement::Assign { name: inc, value: inc_value }] = then_body.as_slice()
                        else {
                            return Ok(false);
                        };
                        if local_index(inc) != Some(sign_local)
                            || !matches!(inc_value,
                                Expression::Binary { operator: BinaryOperator::Add, left, right }
                                    if matches!(left.as_ref(), Expression::Variable(v) if v == inc.as_str())
                                        && crate::analysis::constant_value(right) == Some(1))
                        {
                            return Ok(false);
                        }
                        // else: the carry sequence
                        let [Statement::Assign { name: j_name, value: j_value }, Statement::If { condition: carry_cond, then_body: carry_then, else_body: carry_else }, Statement::Assign { name: copy_name, value: copy_value }] =
                            else_body.as_slice()
                        else {
                            return Ok(false);
                        };
                        if j_name != carry {
                            return Ok(false);
                        }
                        let Expression::Binary { operator: BinaryOperator::Add, left: base, right: one_shift } =
                            j_value
                        else {
                            return Ok(false);
                        };
                        let Expression::Variable(other_name) = base.as_ref() else { return Ok(false) };
                        let Some(other) = local_index(other_name) else { return Ok(false) };
                        let Expression::Binary { operator: BinaryOperator::ShiftLeft, left: one, right: amount } =
                            one_shift.as_ref()
                        else {
                            return Ok(false);
                        };
                        if crate::analysis::constant_value(one) != Some(1) {
                            return Ok(false);
                        }
                        let Expression::Binary { operator: BinaryOperator::Subtract, left: k3, right: by } =
                            amount.as_ref()
                        else {
                            return Ok(false);
                        };
                        let Some(shift_base) =
                            crate::analysis::constant_value(k3).and_then(|k| i16::try_from(k).ok())
                        else {
                            return Ok(false);
                        };
                        if !matches!(by.as_ref(), Expression::Variable(v) if v == guard.name) {
                            return Ok(false);
                        }
                        // if (j < other) l += 1;
                        if !carry_else.is_empty() {
                            return Ok(false);
                        }
                        let Expression::Binary { operator: BinaryOperator::Less, left: jl, right: jr } =
                            carry_cond
                        else {
                            return Ok(false);
                        };
                        if !matches!(jl.as_ref(), Expression::Variable(v) if v == carry)
                            || !matches!(jr.as_ref(), Expression::Variable(v) if local_index(v) == Some(other))
                        {
                            return Ok(false);
                        }
                        let [Statement::Assign { name: inc2, value: inc2_value }] = carry_then.as_slice()
                        else {
                            return Ok(false);
                        };
                        if local_index(inc2) != Some(sign_local)
                            || !matches!(inc2_value,
                                Expression::Binary { operator: BinaryOperator::Add, left, right }
                                    if matches!(left.as_ref(), Expression::Variable(v) if v == inc2.as_str())
                                        && crate::analysis::constant_value(right) == Some(1))
                        {
                            return Ok(false);
                        }
                        // other = j
                        if local_index(copy_name) != Some(other)
                            || !matches!(copy_value, Expression::Variable(v) if v == carry)
                        {
                            return Ok(false);
                        }
                        sign_block = Some(SignBlock::CarryDiamond {
                            local: sign_local,
                            other,
                            equal_bound,
                            shift_base,
                        });
                    }
                    _ => return Ok(false),
                }
                body = &body[1..];
            }
            mutation_statements = body;
        }
        // The self-add's constant must equal the mask synthesis' lis
        // intermediate — mwcc reuses the materialized register (measured:
        // 0x00100000 for the 0xfffff mask). Anything else is unprobed.
        // The constant wraps to its 32-bit value (0xffffffff = li -1).
        let mask_constant = mask_constant as u32 as i32 as i64;
        let needs_temp_early = i16::try_from(mask_constant).is_err();
        let lis_intermediate = ((mask_constant + 0x8000) >> 16) << 16;
        if let Some(SignBlock::Add { constant, .. }) = sign_block {
            // The self-add's constant must CSE the lis intermediate.
            if !needs_temp_early || constant != lis_intermediate || float_guard.is_none() {
                return Ok(false);
            }
        }
        if matches!(sign_block, Some(SignBlock::CarryDiamond { .. })) && float_guard.is_none() {
            return Ok(false);
        }
        if carry_local.is_some() && !matches!(sign_block, Some(SignBlock::CarryDiamond { .. })) {
            return Ok(false);
        }
        // j0's reads beyond the mask shift: the self-add's shift, or the
        // carry diamond's ==K4 and K3-j0.
        let guard_multi_read = sign_block.is_some() || amount_offset != 0;
        // Mutations, then stores.
        enum Mutation {
            Rewrite(i16),
            AndcShift,
            MaskViaScratch { begin: u8, end: u8 },
        }
        let mut mutations: Vec<(usize, Mutation)> = Vec::new();
        let mut tail_cursor = 0usize;
        while let Some(Statement::Assign { name, value }) = mutation_statements.get(tail_cursor) {
            let Some(index) = local_index(name) else { return Ok(false) };
            if mutations.iter().any(|&(seen, _)| seen == index) {
                return Ok(false);
            }
            let mutation = if let Some(constant) = crate::analysis::constant_value(value) {
                let Ok(small) = i16::try_from(constant) else { return Ok(false) };
                Mutation::Rewrite(small)
            } else if let Expression::Binary { operator: BinaryOperator::BitAnd, left, right } = value {
                if !matches!(left.as_ref(), Expression::Variable(v) if v == name.as_str()) {
                    return Ok(false);
                }
                match right.as_ref() {
                    Expression::Unary { operator: UnaryOperator::BitNot, operand }
                        if matches!(operand.as_ref(), Expression::Variable(v) if v == shift) =>
                    {
                        Mutation::AndcShift
                    }
                    other => {
                        let Some((begin, end)) = crate::analysis::constant_value(other)
                            .and_then(crate::analysis::rlwinm_mask)
                        else {
                            return Ok(false);
                        };
                        if float_guard.is_some() {
                            // Unprobed inside a guard (the r0 handoff to the
                            // store would cross the guard bounds).
                            return Ok(false);
                        }
                        Mutation::MaskViaScratch { begin, end }
                    }
                }
            } else {
                return Ok(false);
            };
            mutations.push((index, mutation));
            tail_cursor += 1;
        }
        // At most one rewrite (the li r0 dedupe across two is unmeasured).
        if mutations.iter().filter(|(_, m)| matches!(m, Mutation::Rewrite(_))).count() > 1 {
            return Ok(false);
        }
        // The r0 materialization sinks below the home-writing mutations
        // regardless of source order (measured D3: andc; li r0; stores) —
        // r0's range stays minimal. A CONDITIONAL rewrite (inside the
        // guard) stays in source position: it writes the home.
        if float_guard.is_none() {
            mutations.sort_by_key(|(_, mutation)| matches!(mutation, Mutation::Rewrite(_)));
        }
        // Stores: one per local, its own offset, in local order. With a
        // guard the mutations exhaust its body and the stores follow the
        // guard-If in the outer tail.
        let stores = if float_guard.is_some() {
            if tail_cursor != mutation_statements.len() {
                return Ok(false);
            }
            &tail[2..]
        } else {
            &mutation_statements[tail_cursor..]
        };
        if stores.len() != locals.len() {
            return Ok(false);
        }
        for (statement, &(name, offset)) in stores.iter().zip(&locals) {
            let Statement::Store { target, value } = statement else {
                return Ok(false);
            };
            if crate::frame::pun_word_offset_pub(target, x) != Some(offset)
                || !matches!(value, Expression::Variable(read) if read == name)
            {
                return Ok(false);
            }
        }
        // -- the synthetic position pass --
        use mwcc_vreg::int_alloc::{allocate, Class, Value};
        let needs_temp = i16::try_from(mask_constant).is_err();
        let mut position = 1u32; // 0 = stwu
        let temp_range = needs_temp.then(|| {
            let range = (position, position + 1);
            position += 1;
            range
        });
        let mask_position = position; // li or the addi completing the pair
        position += 1;
        position += 1; // stfd
        // Which locals load: any with a read (test, extract source, andc/mask mutation).
        let has_read = |index: usize| {
            index == test_and_local
                || test_or_local == Some(index)
                || locals[index].0 == guard.source
                || matches!(sign_block, Some(SignBlock::Add { local, .. }) if local == index)
                || matches!(sign_block, Some(SignBlock::CarryDiamond { local, other, .. }) if local == index || other == index)
                || mutations.iter().any(|&(m, ref form)| {
                    m == index
                        && (!matches!(form, Mutation::Rewrite(_)) || float_guard.is_some())
                })
        };
        let mut load_positions: Vec<Option<u32>> = Vec::new();
        for index in 0..locals.len() {
            if has_read(index) {
                load_positions.push(Some(position));
                position += 1;
            } else {
                load_positions.push(None);
            }
        }
        let extract_position = position;
        position += 1;
        let fold_position = position;
        position += 1;
        let sraw_position = position;
        position += 1;
        let and_position = position;
        position += 1;
        let or_position = test_or_local.map(|_| {
            let at = position;
            position += 1;
            at
        });
        let branch_position = position; // bne
        position += 2; // bne + b
        // The inexact-guard block (lfd, lfd, fadd, fcmpo, ble) and the
        // sign-add (cmpwi, bge, sraw2, add).
        if float_guard.is_some() {
            position += 5;
        }
        // The sign block: Add = cmpwi, bge, sraw2, add; CarryDiamond =
        // cmpwi, bge, cmpwi, bne, addi, b, subfic, li, slw, add, cmplw,
        // bge, addi, mr.
        let mut carry_one_range: Option<(u32, u32)> = None;
        let sraw2_position = match &sign_block {
            Some(SignBlock::Add { .. }) => {
                position += 2; // cmpwi + bge
                let at = position;
                position += 2; // sraw2 + add
                Some(at)
            }
            Some(SignBlock::CarryDiamond { .. }) => {
                position += 6; // cmpwi, bge, cmpwi(==K4), bne, addi, b
                let subfic_at = position;
                position += 1;
                let one_at = position;
                position += 1; // li 1
                carry_one_range = Some((one_at, position)); // li..slw
                position += 6; // slw, add, cmplw, bge, addi, mr
                Some(subfic_at) // j0's last read = the subfic
            }
            None => None,
        };
        // Mutations occupy sequential slots (the shared `not` adds one).
        let andc_count = mutations.iter().filter(|(_, m)| matches!(m, Mutation::AndcShift)).count();
        let not_position = (andc_count >= 2).then(|| {
            let at = position;
            position += 1;
            at
        });
        let mut mutation_positions: Vec<u32> = Vec::new();
        for _ in &mutations {
            mutation_positions.push(position);
            position += 1;
        }
        let mut store_positions: Vec<u32> = Vec::new();
        for _ in &locals {
            store_positions.push(position);
            position += 1;
        }
        // -- classify + model --
        let mut values: Vec<Value> = Vec::new();
        let mut tags: Vec<&str> = Vec::new(); // parallel debug tags
        if let Some((lis, addi)) = temp_range {
            // The self-add's constant CSEs the lis intermediate — the
            // temp then lives to the second sraw (measured arm2).
            let last = sraw2_position.unwrap_or(addi);
            values.push(Value { class: Class::Temp, def: lis, last });
            tags.push("temp");
        }
        // With a MULTI-READ guard the fold lands in the home, freeing the
        // r0 timeline — the branch-free mask takes r0 itself (measured
        // arm2: addi r0,r3,-1).
        // ...unless an amount offset (arm3's j0-20) writes r0 inside the
        // mask's live range.
        let mask_in_scratch = guard_multi_read && amount_offset == 0;
        let mask_value_index = if mask_in_scratch {
            None
        } else {
            values.push(Value { class: Class::Mask, def: mask_position, last: sraw_position });
            tags.push("mask");
            Some(values.len() - 1)
        };
        let computed_last = sraw2_position.unwrap_or(fold_position);
        values.push(Value { class: Class::Computed, def: extract_position, last: computed_last });
        tags.push("computed");
        let computed_value_index = values.len() - 1;
        let carry_one_value_index = carry_one_range.map(|(def, last)| {
            values.push(Value { class: Class::Mask, def, last });
            tags.push("carry-one");
            values.len() - 1
        });
        // The shift local: last read = latest of the test and-op and any
        // andc/not mutation.
        let shift_last = if let Some(not_at) = not_position {
            not_at
        } else if let Some(at) = mutations
            .iter()
            .zip(&mutation_positions)
            .filter(|((_, m), _)| matches!(m, Mutation::AndcShift))
            .map(|(_, &at)| at)
            .max()
        {
            at
        } else {
            and_position
        };
        let shift_crosses = shift_last > branch_position;
        let shift_value_index = if shift_crosses {
            values.push(Value { class: Class::Shift, def: sraw_position, last: shift_last });
            tags.push("shift");
            Some(values.len() - 1)
        } else {
            None // r0 (branch-free single use)
        };
        let mut local_value_indices: Vec<Option<usize>> = vec![None; locals.len()];
        for index in 0..locals.len() {
            let Some(load) = load_positions[index] else { continue };
            // The home's last read.
            let mut last = load;
            if locals[index].0 == guard.source {
                last = last.max(extract_position);
            }
            if index == test_and_local {
                last = last.max(and_position);
            }
            if test_or_local == Some(index) {
                last = last.max(or_position.unwrap_or(and_position));
            }
            match &sign_block {
                Some(SignBlock::Add { local, .. }) if *local == index => {
                    // cmpwi + the add read/write the home inside the guard.
                    last = last.max(sraw2_position.expect("sign add") + 1);
                }
                Some(SignBlock::CarryDiamond { local, other, .. })
                    if *local == index || *other == index =>
                {
                    // The homes live through the whole diamond (the mr /
                    // the final addi).
                    last = last.max(sraw2_position.expect("carry") + 7);
                }
                _ => {}
            }
            let mutation = mutations
                .iter()
                .zip(&mutation_positions)
                .find(|((m, _), _)| *m == index);
            let class = match mutation {
                Some(((_, Mutation::Rewrite(_)), _)) if float_guard.is_none() => {
                    // The home dies at its last pre-branch read.
                    Class::LoadDiscarded
                }
                Some(((_, Mutation::Rewrite(_)), _)) => {
                    // A rewrite INSIDE the guard is conditional: the
                    // original flows to the store on the guard-false path.
                    last = last.max(store_positions[index]);
                    Class::LoadSurviving
                }
                Some(((_, Mutation::AndcShift), &at)) => {
                    // andc writes the home; the store reads it.
                    last = last.max(store_positions[index]);
                    let _ = at;
                    Class::LoadSurviving
                }
                Some(((_, Mutation::MaskViaScratch { .. }), &at)) => {
                    // clrlwi reads the home; the store reads r0.
                    last = last.max(at);
                    Class::LoadSurviving
                }
                None => {
                    last = last.max(store_positions[index]);
                    Class::LoadSurviving
                }
            };
            values.push(Value { class, def: load, last });
            tags.push("local");
            local_value_indices[index] = Some(values.len() - 1);
        }
        let registers = allocate(&values);
        let _ = &tags;
        let mask_register = mask_value_index.map(|i| registers[i]).unwrap_or(0);
        let guard_register = registers[computed_value_index];
        let shift_register = shift_value_index.map(|i| registers[i]).unwrap_or(0);
        let home = |index: usize| local_value_indices[index].map(|i| registers[i]);
        // -- emit --
        self.frame_size = 16;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        if needs_temp {
            let temp_register = registers[0];
            let high = ((mask_constant + 0x8000) >> 16) as i16;
            let low = mask_constant as i16;
            self.output.instructions.push(Instruction::load_immediate_shifted(temp_register, high));
            self.output.instructions.push(Instruction::AddImmediate {
                d: mask_register,
                a: temp_register,
                immediate: low,
            });
        } else {
            self.output.instructions.push(Instruction::load_immediate(mask_register, mask_constant as i16));
        }
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 });
        for (index, &(_, offset)) in locals.iter().enumerate() {
            if load_positions[index].is_some() {
                self.output.instructions.push(Instruction::LoadWord {
                    d: home(index).expect("loaded"),
                    a: 1,
                    offset: 8 + offset,
                });
            }
        }
        let source_home = home(local_index(guard.source).expect("validated")).expect("source loads");
        match guard.mask {
            Some(mask) => {
                let rotated = ((32 - guard.shift as u32) % 32) as u8;
                let Some((begin, end)) = crate::analysis::rlwinm_mask(mask) else {
                    return Err(Diagnostic::error("guard mask is not a run (roadmap)"));
                };
                self.output.instructions.push(Instruction::RotateAndMask {
                    a: guard_register,
                    s: source_home,
                    shift: rotated,
                    begin,
                    end,
                });
            }
            None => {
                self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate {
                    a: guard_register,
                    s: source_home,
                    shift: guard.shift,
                });
            }
        }
        let negative = i16::try_from(-guard.offset_k).expect("validated");
        let shift_amount = if guard_multi_read {
            // Multiple j0 reads: the -K lands in the home; an amount
            // offset (arm3's j0-20) folds separately into r0.
            self.output.instructions.push(Instruction::AddImmediate {
                d: guard_register,
                a: guard_register,
                immediate: negative,
            });
            if amount_offset != 0 {
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 0,
                    a: guard_register,
                    immediate: i16::try_from(-amount_offset).expect("validated"),
                });
                0
            } else {
                guard_register
            }
        } else {
            self.output.instructions.push(Instruction::AddImmediate { d: 0, a: guard_register, immediate: negative });
            0
        };
        if logical_shift {
            self.output.instructions.push(Instruction::ShiftRightWord {
                a: shift_register,
                s: mask_register,
                b: shift_amount,
            });
        } else {
            self.output.instructions.push(Instruction::ShiftRightAlgebraicWord {
                a: shift_register,
                s: mask_register,
                b: shift_amount,
            });
        }
        // The test.
        let and_home = home(test_and_local).expect("test local loads");
        if let Some(or_local) = test_or_local {
            self.output.instructions.push(Instruction::And { a: 0, s: and_home, b: shift_register });
            self.output.instructions.push(Instruction::OrRecord {
                a: 0,
                s: home(or_local).expect("test local loads"),
                b: 0,
            });
        } else {
            self.output.instructions.push(Instruction::AndRecord { a: 0, s: and_home, b: shift_register });
        }
        let continuation = self.fresh_label();
        let epilogue = self.fresh_label();
        let join = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, continuation); // bne — skip the return
        self.emit_branch_to(epilogue);
        self.bind_label(continuation);
        if let Some((huge, zero)) = float_guard {
            // The nested inexact guard (the G2 recipe): huge/0.0 pool-load
            // back-to-back into f2/f0, fadd clobbers the spilled f1, ble
            // chains to the join.
            self.load_double_constant(2, huge);
            self.load_double_constant(0, zero);
            self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 2, b: 1 });
            self.output.instructions.push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
            self.emit_branch_conditional_to(4, 1, join);
        }
        match &sign_block {
            Some(SignBlock::Add { local, .. }) => {
                // `if (l < 0) l += C2 >> j0` — C2 reuses the lis intermediate.
                let register = home(*local).expect("sign local loads");
                let temp_register = registers[0];
                let skip = self.fresh_label();
                self.output.instructions.push(Instruction::CompareWordImmediate { a: register, immediate: 0 });
                self.emit_branch_conditional_to(4, 0, skip); // bge
                self.output.instructions.push(Instruction::ShiftRightAlgebraicWord {
                    a: 0,
                    s: temp_register,
                    b: guard_register,
                });
                self.output.instructions.push(Instruction::Add { d: register, a: register, b: 0 });
                self.bind_label(skip);
            }
            Some(SignBlock::CarryDiamond { local, other, equal_bound, shift_base }) => {
                // `if (l < 0) { if (j0 == K4) l += 1; else { j = other +
                // (1 << (K3 - j0)); if (j < other) l += 1; other = j; } }`
                // — j lives in r0; the ONE constant takes a model register
                // (arm3: the dead mask's r3).
                let register = home(*local).expect("sign local loads");
                let other_register = home(*other).expect("carry source loads");
                let one_register = carry_one_value_index
                    .map(|i| registers[i])
                    .expect("carry one allocated");
                let continue_at = self.fresh_label(); // the trailing mutations
                let else_at = self.fresh_label();
                let no_carry = self.fresh_label();
                self.output.instructions.push(Instruction::CompareWordImmediate { a: register, immediate: 0 });
                self.emit_branch_conditional_to(4, 0, continue_at); // bge — skip the diamond
                self.output.instructions.push(Instruction::CompareWordImmediate {
                    a: guard_register,
                    immediate: *equal_bound,
                });
                self.emit_branch_conditional_to(4, 2, else_at); // bne
                self.output.instructions.push(Instruction::AddImmediate { d: register, a: register, immediate: 1 });
                self.emit_branch_to(continue_at);
                self.bind_label(else_at);
                self.output.instructions.push(Instruction::SubtractFromImmediate {
                    d: 0,
                    a: guard_register,
                    immediate: *shift_base,
                });
                self.output.instructions.push(Instruction::load_immediate(one_register, 1));
                self.output.instructions.push(Instruction::ShiftLeftWord { a: 0, s: one_register, b: 0 });
                self.output.instructions.push(Instruction::Add { d: 0, a: other_register, b: 0 });
                self.output.instructions.push(Instruction::CompareLogicalWord { a: 0, b: other_register });
                self.emit_branch_conditional_to(4, 0, no_carry); // bge — unsigned no-carry
                self.output.instructions.push(Instruction::AddImmediate { d: register, a: register, immediate: 1 });
                self.bind_label(no_carry);
                self.output.instructions.push(Instruction::move_register(other_register, 0));
                self.bind_label(continue_at);
            }
            None => {}
        }
        // Mutations (the shared `not` precedes the first andc pair).
        if not_position.is_some() {
            self.output.instructions.push(Instruction::Nor { a: 0, s: shift_register, b: shift_register });
        }
        for (index, mutation) in &mutations {
            let index = *index;
            match mutation {
                Mutation::Rewrite(constant) => {
                    // Conditional (guarded) rewrites write the HOME — the
                    // original flows to the store on the guard-false path.
                    let target = if float_guard.is_some() {
                        home(index).expect("conditional rewrite loads")
                    } else {
                        0
                    };
                    self.output.instructions.push(Instruction::load_immediate(target, *constant));
                }
                Mutation::AndcShift => {
                    let register = home(index).expect("loaded");
                    if not_position.is_some() {
                        self.output.instructions.push(Instruction::And { a: register, s: register, b: 0 });
                    } else {
                        self.output.instructions.push(Instruction::AndComplement {
                            a: register,
                            s: register,
                            b: shift_register,
                        });
                    }
                }
                Mutation::MaskViaScratch { begin, end } => {
                    self.output.instructions.push(Instruction::RotateAndMask {
                        a: 0,
                        s: home(index).expect("loaded"),
                        shift: 0,
                        begin: *begin,
                        end: *end,
                    });
                }
            }
        }
        // Stores (the guard's ble lands here): surviving homes store
        // themselves; UNCONDITIONAL rewrites and mask-via-scratch store
        // from r0.
        self.bind_label(join);
        for (index, &(_, offset)) in locals.iter().enumerate() {
            let from_scratch = float_guard.is_none()
                && mutations.iter().any(|&(m, ref form)| {
                    m == index && !matches!(form, Mutation::AndcShift)
                });
            let register = if from_scratch { 0 } else { home(index).map(|r| r).unwrap_or(0) };
            self.output.instructions.push(Instruction::StoreWord { s: register, a: 1, offset: 8 + offset });
        }
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 8 });
        self.bind_label(epilogue);
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // Pre-pool labels: one plus one per LOADED local (measured V1c
        // @7 and W11 @7 with one load, V1b @8 with two — the never-read
        // store-only local costs nothing), plus one for the shared `not`
        // temp (W10 @9).
        self.output.anonymous_label_bump += 1
            + load_positions.iter().filter(|p| p.is_some()).count() as u32
            + not_position.is_some() as u32
            + 2 * float_guard.is_some() as u32
            + match &sign_block {
                Some(SignBlock::Add { .. }) => 2,
                // Three inner conditions (sign, ==K4, the carry compare)
                // at two each, one else arm, one for the ONE temp
                // (measured @18 on the arm3 object).
                Some(SignBlock::CarryDiamond { .. }) => 8,
                None => 0,
            };
        Ok(true)
    }

    /// The computed GUARD local `j0 = ((punned >> S) [& M]) - K` shared by
    /// the punned-writeback family (parsed once, consumed by the branch
    /// and select paths).
    pub(crate) fn try_punned_guard_writeback(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        if function.return_type != Type::Double
            || !function.guards.is_empty()
            || function_makes_call(function)
            || self.non_leaf
        {
            return Ok(false);
        }
        let Some(Expression::Variable(returned)) = &function.return_expression else {
            return Ok(false);
        };
        let Some(first) = function.parameters.first() else {
            return Ok(false);
        };
        if first.parameter_type != Type::Double || returned != &first.name {
            return Ok(false);
        }
        let x = first.name.as_str();
        // Every local: an int punned read of x at a distinct word offset —
        // or ONE computed GUARD local `j0 = ((punned >> S) [& M]) - K`
        // read only by the outer condition (s_floor's exponent extract).
        let mut locals: Vec<(&str, i16)> = Vec::new();
        let mut guard_local: Option<GuardLocal> = None;
        for local in &function.locals {
            if local.declared_type != Type::Int || local.array_length.is_some() {
                return Ok(false);
            }
            let Some(init) = &local.initializer else {
                return Ok(false);
            };
            if let Some(offset) = crate::frame::pun_word_offset_pub(init, x) {
                if locals.iter().any(|&(_, seen)| seen == offset) {
                    return Ok(false);
                }
                locals.push((local.name.as_str(), offset));
                continue;
            }
            // The computed guard local: strip a trailing `- K`.
            if guard_local.is_some() {
                return Ok(false);
            }
            let (core, offset_k) = match init {
                Expression::Binary { operator: BinaryOperator::Subtract, left, right } => {
                    let Some(k) = crate::analysis::constant_value(right) else {
                        return Ok(false);
                    };
                    (left.as_ref(), k)
                }
                other => (other, 0),
            };
            // `(punned >> S) & M` or bare `punned >> S`.
            let (shifted, mask) = match core {
                Expression::Binary { operator: BinaryOperator::BitAnd, left, right } => {
                    let Some(mask) = crate::analysis::constant_value(right) else {
                        return Ok(false);
                    };
                    (left.as_ref(), Some(mask))
                }
                other => (other, None),
            };
            let Expression::Binary { operator: BinaryOperator::ShiftRight, left, right } = shifted else {
                return Ok(false);
            };
            let Expression::Variable(source) = left.as_ref() else {
                return Ok(false);
            };
            let Some(shift) = crate::analysis::constant_value(right) else {
                return Ok(false);
            };
            let Ok(shift) = u8::try_from(shift) else {
                return Ok(false);
            };
            guard_local = Some(GuardLocal {
                name: local.name.as_str(),
                source,
                shift,
                mask,
                offset_k,
            });
        }
        if locals.is_empty() || locals.len() > 2 {
            return Ok(false);
        }
        if let Some(guard) = &guard_local {
            // The source must be a punned local; the guard local reads
            // nowhere else (its home holds only the pre-offset value).
            if !locals.iter().any(|&(name, _)| name == guard.source) {
                return Ok(false);
            }
        }
        // statements = [If{cond, [early-return-x if]? [mutations]}] + one
        // punned store per local writing it back to ITS offset.
        let (Some(Statement::If { condition, then_body, else_body }), stores) =
            (function.statements.first(), &function.statements[1..])
        else {
            return Ok(false);
        };
        if stores.len() != locals.len() {
            return Ok(false);
        }
        // The BLOCK: a recursive tree over the measured statement forms —
        // constant/high/self-mask mutations, nested no-else guards (chained
        // to the join), if/ELSE-IF arms (branch-over + b join), and
        // mid-chain `return x` (straight to the epilogue). Validated here;
        // emitted by the recursive walker below.
        let block: &[Statement] = then_body;
        fn validate_block(
            block: &[Statement],
            locals: &[(&str, i16)],
            x: &str,
            mutated: &mut Vec<usize>,
            conditions: &mut usize,
            arms: &mut usize,
        ) -> bool {
            for statement in block {
                match statement {
                    Statement::Assign { name, value } => {
                        let Some(index) = locals.iter().position(|&(local, _)| local == name.as_str()) else {
                            return false;
                        };
                        if !mutated.contains(&index) {
                            mutated.push(index);
                        }
                        // The chain `i0 = i1 = C`: both locals mutate from
                        // one small constant.
                        if let Expression::Assign { target, value: inner_value } = value {
                            let Expression::Variable(inner) = target.as_ref() else {
                                return false;
                            };
                            let Some(inner_index) =
                                locals.iter().position(|&(local, _)| local == inner.as_str())
                            else {
                                return false;
                            };
                            if !mutated.contains(&inner_index) {
                                mutated.push(inner_index);
                            }
                            if !crate::analysis::constant_value(inner_value)
                                .map(|constant| i16::try_from(constant).is_ok())
                                .unwrap_or(false)
                            {
                                return false;
                            }
                            continue;
                        }
                        let constant_ok = crate::analysis::constant_value(value)
                            .map(|constant| {
                                i16::try_from(constant).is_ok()
                                    || (constant & 0xffff == 0 && u32::try_from(constant).is_ok())
                            })
                            .unwrap_or(false);
                        if constant_ok {
                            continue;
                        }
                        let mask_ok = matches!(
                            value,
                            Expression::Binary { operator: BinaryOperator::BitAnd, left, right }
                                if matches!(left.as_ref(), Expression::Variable(read) if read == name.as_str())
                                    && crate::analysis::constant_value(right)
                                        .and_then(crate::analysis::rlwinm_mask)
                                        .is_some()
                        );
                        if !mask_ok {
                            return false;
                        }
                    }
                    Statement::Return(Some(Expression::Variable(value))) if value == x => {}
                    Statement::Return(Some(Expression::Binary {
                        operator: BinaryOperator::Add,
                        left,
                        right,
                    })) if matches!((left.as_ref(), right.as_ref()),
                        (Expression::Variable(a), Expression::Variable(b)) if a == x && b == x) => {}
                    Statement::If { condition: _, then_body, else_body } => {
                        *conditions += 1;
                        if !validate_block(then_body, locals, x, mutated, conditions, arms) {
                            return false;
                        }
                        if !else_body.is_empty() {
                            *arms += 1;
                            if !validate_block(else_body, locals, x, mutated, conditions, arms) {
                                return false;
                            }
                        }
                    }
                    _ => return false,
                }
            }
            true
        }
        let mut mutated: Vec<usize> = Vec::new();
        let mut inner_conditions = 0usize;
        let mut else_arms = 0usize;
        if !validate_block(block, &locals, x, &mut mutated, &mut inner_conditions, &mut else_arms) {
            return Ok(false);
        }
        if !else_body.is_empty() {
            else_arms += 1;
            if !validate_block(else_body, &locals, x, &mut mutated, &mut inner_conditions, &mut else_arms) {
                return Ok(false);
            }
        }
        if mutated.is_empty() {
            return Ok(false);
        }
        fn block_reads(block: &[Statement], name: &str) -> usize {
            block
                .iter()
                .map(|statement| match statement {
                    Statement::Assign { value, .. } => count_name_occurrences(value, name),
                    Statement::Return(Some(value)) => count_name_occurrences(value, name),
                    Statement::If { condition, then_body, else_body } => {
                        count_name_occurrences(condition, name)
                            + block_reads(then_body, name)
                            + block_reads(else_body, name)
                    }
                    _ => 0,
                })
                .sum()
        }
        fn block_condition_reads(block: &[Statement], name: &str) -> usize {
            block
                .iter()
                .map(|statement| match statement {
                    Statement::If { condition, then_body, else_body } => {
                        count_name_occurrences(condition, name)
                            + block_condition_reads(then_body, name)
                            + block_condition_reads(else_body, name)
                    }
                    _ => 0,
                })
                .sum()
        }
        fn block_self_masks(block: &[Statement], name: &str) -> bool {
            block.iter().any(|statement| match statement {
                Statement::Assign { name: target, value } => {
                    target.as_str() == name && crate::analysis::constant_value(value).is_none()
                }
                Statement::If { then_body, else_body, .. } => {
                    block_self_masks(then_body, name) || block_self_masks(else_body, name)
                }
                _ => false,
            })
        }
        // The writebacks: each local stored to its own offset, in order.
        for (statement, &(name, offset)) in stores.iter().zip(&locals) {
            let Statement::Store { target, value } = statement else {
                return Ok(false);
            };
            if crate::frame::pun_word_offset_pub(target, x) != Some(offset) {
                return Ok(false);
            }
            if !matches!(value, Expression::Variable(read) if read == name) {
                return Ok(false);
            }
        }
        // The FLOAT-compare guard: `HUGE + x > 0.0` (the static const
        // folded to a literal upstream) — measured: lfd huge BEFORE the
        // spill, fadd clobbering f1 (x is spilled), the pooled 0.0, the
        // loads woven before the fcmpo, ble skip.
        let float_guard: Option<(u64, u64)> = match condition {
            Expression::Binary { operator: BinaryOperator::Greater, left, right } => {
                let zero = match right.as_ref() {
                    Expression::FloatLiteral(value) => Some(value.to_bits()),
                    _ => None,
                };
                let huge = match left.as_ref() {
                    Expression::Binary { operator: BinaryOperator::Add, left: huge, right: xvar } => {
                        if matches!(xvar.as_ref(), Expression::Variable(name) if name == x) {
                            match huge.as_ref() {
                                Expression::FloatLiteral(value) => Some(value.to_bits()),
                                _ => None,
                            }
                        } else {
                            None
                        }
                    }
                    _ => None,
                };
                match (huge, zero) {
                    (Some(huge), Some(zero)) if f64::from_bits(zero) == 0.0 => Some((huge, zero)),
                    _ => None,
                }
            }
            _ => None,
        };
        if float_guard.is_some() && guard_local.is_some() {
            return Ok(false);
        }
        // The BRANCHLESS ZERO-SELECT: `if (j0 cmp K) p = A; else p = B;`
        // where one arm is 0 — 2.6 if-converts to mask algebra with no
        // branches at all (measured L3/L4/S2/S3/R1/R2/R3).
        if let Some(guard) = &guard_local {
            if locals.len() == 1
                && self.try_punned_zero_select(&locals, guard, condition, block, else_body)?
            {
                return Ok(true);
            }
            if locals.len() == 1
                && self.try_punned_hoisted_overwrite(&locals, guard, condition, block, else_body)?
            {
                return Ok(true);
            }
        }
        // The guard-local condition: `j0 < C` only (measured), with j0
        // read nowhere else in the function.
        let mut guard_compare: Option<(i16, i64)> = None;
        if let Some(guard) = &guard_local {

            let Expression::Binary { operator: BinaryOperator::Less, left, right } = condition else {
                return Ok(false);
            };
            if !matches!(left.as_ref(), Expression::Variable(name) if name == guard.name) {
                return Ok(false);
            }
            let Some(bound) = crate::analysis::constant_value(right) else {
                return Ok(false);
            };
            let Ok(bound) = i16::try_from(bound) else {
                return Ok(false);
            };
            let condition_reads = count_name_occurrences(condition, guard.name)
                + block_condition_reads(block, guard.name)
                + block_condition_reads(else_body, guard.name);
            let non_condition = block_reads(block, guard.name) - block_condition_reads(block, guard.name)
                + block_reads(else_body, guard.name)
                - block_condition_reads(else_body, guard.name)
                + stores
                    .iter()
                    .map(|statement| match statement {
                        Statement::Store { target, value } => {
                            count_name_occurrences(target, guard.name)
                                + count_name_occurrences(value, guard.name)
                        }
                        _ => 0,
                    })
                    .sum::<usize>();
            if non_condition != 0 {
                return Ok(false);
            }
            if condition_reads == 1 {
                // Single read: the -K folds into the scratch compare.
                guard_compare = Some((bound, guard.offset_k));
            }
            // Multi-read: the home takes the FULL value (addi into the
            // home, measured L1) and every condition reads it plainly.
        }
        // THE LIVENESS RULE (refines the old scratch rule; measured
        // P1/L1/L2 plus the eight 1054 shapes): r0 is denied to the
        // punned locals only when the r0 scratch is actually WRITTEN
        // (the single-read guard fold, a record-form idiom) while an
        // ORIGINAL loaded value is still live past the scratch point —
        // an arm reads it, or some writeback-reaching path skips
        // reassigning it so the stw reads it. L1's multi-read guard
        // (addi into the home, no fold) leaves r0 free; L2's
        // else-returns shape reassigns on every surviving path.
        fn condition_needs_scratch(condition: &Expression) -> bool {
            !matches!(
                condition,
                Expression::Variable(_)
                    | Expression::Binary { left: _, right: _, .. }
                        if matches!(condition, Expression::Variable(_))
                            || matches!(
                                condition,
                                Expression::Binary { left, right, .. }
                                    if matches!(left.as_ref(), Expression::Variable(_))
                                        && matches!(right.as_ref(), Expression::IntegerLiteral(_))
                            )
            )
        }
        fn block_needs_scratch(block: &[Statement]) -> bool {
            block.iter().any(|statement| match statement {
                Statement::If { condition, then_body, else_body } => {
                    condition_needs_scratch(condition)
                        || block_needs_scratch(then_body)
                        || block_needs_scratch(else_body)
                }
                _ => false,
            })
        }
        // Every leaf path either reassigns the local or leaves the
        // function before the writeback.
        fn covered(block: &[Statement], name: &str) -> bool {
            block.iter().any(|statement| match statement {
                Statement::Assign { name: target, .. } => target.as_str() == name,
                Statement::Return(_) => true,
                Statement::If { then_body, else_body, .. } => {
                    !else_body.is_empty() && covered(then_body, name) && covered(else_body, name)
                }
                _ => false,
            })
        }
        let scratch_written =
            guard_compare.is_some() || block_needs_scratch(block) || block_needs_scratch(else_body);
        let any_original_survives = locals.iter().any(|&(name, _)| {
            block_reads(block, name) + block_reads(else_body, name) > 0
                || !(covered(block, name) && !else_body.is_empty() && covered(else_body, name))
        });
        let scratch_taken = scratch_written && any_original_survives;
        let mut next_general = if guard_local.is_some() { 4u8 } else { 3u8 };
        let guard_register = 3u8;
        let mut registers: Vec<u8> = Vec::new();
        let mut r0_used = scratch_taken;
        for _ in &locals {
            if !r0_used {
                registers.push(0);
                r0_used = true;
            } else {
                registers.push(next_general);
                next_general += 1;
            }
        }
        // Live int params below the allocated range are unmeasured — every
        // capture either had none or had them freed by the outer condition.
        let top = registers.iter().copied().max().unwrap_or(0);
        for parameter in &function.parameters {
            if parameter.parameter_type == Type::Double {
                continue;
            }
            let Some(register) = self.lookup_general(&parameter.name) else {
                return Ok(false);
            };
            if register <= top && count_name_occurrences(condition, &parameter.name) == 0 {
                return Ok(false);
            }
        }
        // -- commit --
        self.frame_size = 16;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        let hoisted = if guard_local.is_none() && float_guard.is_none() {
            Some(self.emit_condition_test(condition)?)
        } else {
            None
        };
        if let Some((huge, _)) = float_guard {
            // The huge pool load precedes the spill (measured).
            self.load_double_constant(0, huge);
        }
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 });
        if let Some((_, zero)) = float_guard {
            // fadd f1,f0,f1 clobbers x's register — the spill covers the
            // tail's reload; the pooled 0.0 loads before the int reads.
            self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 0, b: 1 });
            self.load_double_constant(0, zero);
        }
        for (index, &(_, offset)) in locals.iter().enumerate() {
            self.output.instructions.push(Instruction::LoadWord { d: registers[index], a: 1, offset: 8 + offset });
        }
        if float_guard.is_some() {
            // No has_float_branch bump: the writeback's fcmpo+ble counts
            // only the arm's own labels (measured: pool @50 vs +3's @53).
            self.output.instructions.push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
        }
        if let Some(guard) = &guard_local {
            // The guard local computes AFTER the loads: the fused shift+mask
            // (rlwinm) or plain srawi into its home; a SINGLE condition read
            // folds the -K into the scratch compare, MULTIPLE reads land the
            // full value in the home (measured L1's addi r3,r3,-1023).
            let source_register = locals
                .iter()
                .position(|&(name, _)| name == guard.source)
                .map(|index| registers[index])
                .expect("source is punned");
            match guard.mask {
                Some(mask) => {
                    let rotated = (32 - guard.shift as u32) % 32;
                    let Some((begin, end)) = crate::analysis::rlwinm_mask(mask) else {
                        return Err(Diagnostic::error("guard mask is not a run (roadmap)"));
                    };
                    self.output.instructions.push(Instruction::RotateAndMask {
                        a: guard_register,
                        s: source_register,
                        shift: rotated as u8,
                        begin,
                        end,
                    });
                }
                None => {
                    self.output.instructions.push(Instruction::ShiftRightAlgebraicImmediate {
                        a: guard_register,
                        s: source_register,
                        shift: guard.shift,
                    });
                }
            }
            if guard_compare.is_none() && guard.offset_k != 0 {
                let Ok(negative) = i16::try_from(-guard.offset_k) else {
                    return Err(Diagnostic::error("guard offset beyond i16 (roadmap)"));
                };
                self.output.instructions.push(Instruction::AddImmediate {
                    d: guard_register,
                    a: guard_register,
                    immediate: negative,
                });
            }
        }
        let join = self.fresh_label();
        let epilogue = self.fresh_label();
        let outer_laddered = !else_body.is_empty() || (guard_local.is_some() && guard_compare.is_none());
        if outer_laddered && !(guard_local.is_some() && guard_compare.is_none()) {
            // Laddered forms are BYTE-verified only for the multi-read
            // guard (L1: the addi lands in the home and every condition
            // reads it plainly). A single-read fold or plain/float outer
            // condition inside the walker is unfitted (L2's inverted
            // else-return, the hoisted double-emission) — defer.
            return Ok(false);
        }
        if !outer_laddered {
            let (options, condition_bit) = match hoisted {
                Some(encoding) => encoding,
                None if float_guard.is_some() => (4, 1), // ble — the > 0.0 skip
                None => {
                    let (bound, offset_k) = guard_compare.expect("gated above");
                    if offset_k != 0 {
                        let Ok(negative) = i16::try_from(-offset_k) else {
                            return Err(Diagnostic::error("guard offset beyond i16 (roadmap)"));
                        };
                        if bound == 0 {
                            // A zero bound records the fold itself — the
                            // compare is free (measured G1: addic. r0; bge).
                            self.output.instructions.push(Instruction::AddImmediateCarryingRecord {
                                d: 0,
                                a: guard_register,
                                immediate: negative,
                            });
                        } else {
                            self.output.instructions.push(Instruction::AddImmediate {
                                d: 0,
                                a: guard_register,
                                immediate: negative,
                            });
                            self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: bound });
                        }
                    } else {
                        self.output.instructions.push(Instruction::CompareWordImmediate {
                            a: guard_register,
                            immediate: bound,
                        });
                    }
                    (4, 0) // bge — the Less guard's skip sense
                }
            };
            self.emit_branch_conditional_to(options, condition_bit, join);
        }
        // The punned locals resolve in every inner condition through
        // temporary locations at their scratch registers, installed around
        // the whole block walk.
        let mut saved: Vec<(String, Option<crate::generator::Location>)> = Vec::new();
        for (index, &(name, _)) in locals.iter().enumerate() {
            saved.push((
                name.to_string(),
                self.locations.insert(
                    name.to_string(),
                    crate::generator::Location {
                        class: ValueClass::General,
                        register: registers[index],
                        signed: true,
                        width: 32,
                        pointee: None,
                        stride: None,
                    },
                ),
            ));
        }
        let mut bindings: Vec<(String, u8)> = locals
            .iter()
            .enumerate()
            .map(|(index, &(name, _))| (name.to_string(), registers[index]))
            .collect();
        if let Some(guard) = &guard_local {
            bindings.push((guard.name.to_string(), guard_register));
            saved.push((
                guard.name.to_string(),
                self.locations.insert(
                    guard.name.to_string(),
                    crate::generator::Location {
                        class: ValueClass::General,
                        register: guard_register,
                        signed: true,
                        width: 32,
                        pointee: None,
                        stride: None,
                    },
                ),
            ));
        }
        let outer_statement = [function.statements[0].clone()];
        let walked = if outer_laddered {
            self.emit_writeback_block(&outer_statement, &bindings, join, epilogue)
        } else {
            self.emit_writeback_block(block, &bindings, join, epilogue)
        };
        for (name, previous) in saved {
            match previous {
                Some(location) => {
                    self.locations.insert(name, location);
                }
                None => {
                    self.locations.remove(&name);
                }
            }
        }
        walked?;
        self.bind_label(join);
        for (index, &(_, offset)) in locals.iter().enumerate() {
            self.output.instructions.push(Instruction::StoreWord { s: registers[index], a: 1, offset: 8 + offset });
        }
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 8 });
        self.bind_label(epilogue);
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // Pre-pool labels: the outer if pair, one per additional punned
        // local, two per inner condition, one per else arm (measured up to
        // the two-condition/one-arm forms; deeper shapes iterate). The
        // laddered outer costs one more (measured L1: @12 vs the +6
        // formula's @11), and each `return x+x` costs one (its expression
        // temp — measured M1 @16 and M4 @15 against the formula's -1).
        fn count_fadd_returns(block: &[Statement]) -> u32 {
            block
                .iter()
                .map(|statement| match statement {
                    Statement::Return(Some(Expression::Binary {
                        operator: BinaryOperator::Add,
                        ..
                    })) => 1,
                    Statement::If { then_body, else_body, .. } => {
                        count_fadd_returns(then_body) + count_fadd_returns(else_body)
                    }
                    _ => 0,
                })
                .sum()
        }
        self.output.anonymous_label_bump += 1
            + locals.len() as u32
            + 2 * inner_conditions as u32
            + else_arms as u32
            + outer_laddered as u32
            + count_fadd_returns(block)
            + count_fadd_returns(else_body);
        Ok(true)
    }

    /// The writeback block WALKER: mutations, tail guards chaining to the
    /// join, if/ELSE-IF arms, and mid-chain `return x` straight to the
    /// epilogue (measured: the N1/N2 nested captures).
    fn emit_writeback_block(
        &mut self,
        block: &[Statement],
        bindings: &[(String, u8)],
        join: mwcc_vreg::Label,
        epilogue: mwcc_vreg::Label,
    ) -> Compilation<()> {
        use mwcc_syntax_trees::Statement;
        let mut index = 0usize;
        while index < block.len() {
            let statement = &block[index];
            let last = index + 1 == block.len();
            match statement {
                Statement::Assign { name, value } => {
                    let register = bindings
                        .iter()
                        .find(|(local, _)| local == name)
                        .map(|&(_, register)| register)
                        .expect("validated");
                    // The chain `i0 = i1 = C` assigns right-to-left: the
                    // inner local first, then the outer from the same
                    // constant (measured G1: li r5,0; li r4,0).
                    if let Expression::Assign { target, value: inner_value } = value {
                        let Expression::Variable(inner) = target.as_ref() else {
                            return Err(Diagnostic::error("chained store target beyond the walker (roadmap)"));
                        };
                        let inner_register = bindings
                            .iter()
                            .find(|(local, _)| local == inner)
                            .map(|&(_, register)| register)
                            .expect("validated");
                        let constant = crate::analysis::constant_value(inner_value).expect("validated");
                        let small = i16::try_from(constant).expect("validated");
                        self.output.instructions.push(Instruction::load_immediate(inner_register, small));
                        self.output.instructions.push(Instruction::load_immediate(register, small));
                        index += 1;
                        continue;
                    }
                    if let Some(constant) = crate::analysis::constant_value(value) {
                        if let Ok(small) = i16::try_from(constant) {
                            self.output.instructions.push(Instruction::load_immediate(register, small));
                        } else {
                            self.output
                                .instructions
                                .push(Instruction::load_immediate_shifted(register, (constant >> 16) as i16));
                        }
                    } else if let Expression::Binary { operator: BinaryOperator::BitAnd, right, .. } = value {
                        let mask = crate::analysis::constant_value(right).expect("validated");
                        let (begin, end) = crate::analysis::rlwinm_mask(mask).expect("validated");
                        self.output.instructions.push(Instruction::RotateAndMask {
                            a: register,
                            s: register,
                            shift: 0,
                            begin,
                            end,
                        });
                    } else {
                        return Err(Diagnostic::error("writeback mutation beyond the walker (roadmap)"));
                    }
                }
                Statement::Return(Some(value)) => {
                    // `return x+x` raises inexact/inf via fadd before the
                    // epilogue (measured M1: fadd f1,f1,f1; b epi); f1 is
                    // never clobbered on walker paths, so a plain return
                    // is the bare branch.
                    if let Expression::Binary { operator: BinaryOperator::Add, left, right } = value {
                        if matches!((left.as_ref(), right.as_ref()),
                            (Expression::Variable(a), Expression::Variable(b)) if a == b)
                        {
                            self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 1, b: 1 });
                        }
                    }
                    self.emit_branch_to(epilogue);
                }
                Statement::If { condition, then_body, else_body } => {
                    if let Some((huge, zero)) = float_guard_condition(condition) {
                        // The NESTED inexact guard (measured G2): huge and
                        // 0.0 pool-load back-to-back into f2/f0, the fadd
                        // clobbers f1 (x stays spilled), ble chains to the
                        // join like any tail guard.
                        if !else_body.is_empty() || !last {
                            return Err(Diagnostic::error(
                                "a non-tail float guard in the walker (roadmap)",
                            ));
                        }
                        self.load_double_constant(2, huge);
                        self.load_double_constant(0, zero);
                        self.output.instructions.push(Instruction::FloatAddDouble { d: 1, a: 2, b: 1 });
                        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
                        self.emit_branch_conditional_to(4, 1, join);
                        self.emit_writeback_block(then_body, bindings, join, epilogue)?;
                        index += 1;
                        continue;
                    }
                    let (options, condition_bit) = self.emit_condition_test(condition)?;
                    if let [Statement::Return(Some(_))] = else_body.as_slice() {
                        if matches!(then_body.last(), Some(Statement::Return(_))) {
                            // BOTH arms leave: the else's b-epilogue folds
                            // into the skip branch itself (measured M1:
                            // cmpwi; bne EPI; fadd; b EPI).
                            self.emit_branch_conditional_to(options, condition_bit, epilogue);
                            self.emit_writeback_block(then_body, bindings, join, epilogue)?;
                            index += 1;
                            continue;
                        }
                        // The then FALLS to the join: the arms swap — the
                        // taken sense enters the then arm, the return lands
                        // inline as b epilogue (measured L2: blt; b epi;
                        // muts).
                        let continuation = self.fresh_label();
                        self.emit_branch_conditional_to(options ^ 8, condition_bit, continuation);
                        self.emit_branch_to(epilogue);
                        self.bind_label(continuation);
                        self.emit_writeback_block(then_body, bindings, join, epilogue)?;
                        index += 1;
                        continue;
                    }
                    if !else_body.is_empty() {
                        // if/ELSE-IF: branch over the then arm; b join after
                        // it — omitted when every then path already leaves
                        // (measured M1: fadd; b epi; ELSE with no b join).
                        fn block_leaves(block: &[Statement]) -> bool {
                            match block.last() {
                                Some(Statement::Return(_)) => true,
                                Some(Statement::If { then_body, else_body, .. }) => {
                                    !else_body.is_empty()
                                        && block_leaves(then_body)
                                        && block_leaves(else_body)
                                }
                                _ => false,
                            }
                        }
                        let else_label = self.fresh_label();
                        self.emit_branch_conditional_to(options, condition_bit, else_label);
                        self.emit_writeback_block(then_body, bindings, join, epilogue)?;
                        if !block_leaves(then_body) {
                            self.emit_branch_to(join);
                        }
                        self.bind_label(else_label);
                        self.emit_writeback_block(else_body, bindings, join, epilogue)?;
                    } else if let [Statement::Return(Some(_))] = then_body.as_slice() {
                        // The mid-chain return: skip to the continuation.
                        // The recursion supplies the return emission (the
                        // bare b epilogue, or fadd first for x+x).
                        let continuation = self.fresh_label();
                        self.emit_branch_conditional_to(options, condition_bit, continuation);
                        self.emit_writeback_block(then_body, bindings, join, epilogue)?;
                        self.bind_label(continuation);
                    } else if last {
                        // A tail guard chains to the block's join.
                        self.emit_branch_conditional_to(options, condition_bit, join);
                        self.emit_writeback_block(then_body, bindings, join, epilogue)?;
                    } else {
                        return Err(Diagnostic::error("a non-tail guard in the writeback (roadmap)"));
                    }
                }
                _ => return Err(Diagnostic::error("writeback statement beyond the walker (roadmap)")),
            }
            index += 1;
        }
        Ok(())
    }

    /// GUARD-BLOCK MUTATIONS (the s_floor skeleton, fire 377): a chain of
    /// nested no-else ifs whose innermost body only ASSIGNS constants to int
    /// params, followed by a return expression. Every guard branches to ONE
    /// join; the block mutates the params in their own registers; the join
    /// computes the return (measured: `if(c){i0=0;i1=0;} return i0|i1` =
    /// cmpwi; beq J; li; li; J: or; blr — and the nested form re-tests each
    /// guard to the same join).
    pub(crate) fn try_guard_block_mutations(&mut self, function: &Function) -> Compilation<bool> {
        if !function.guards.is_empty() || !function.locals.is_empty() || function_makes_call(function) {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let Some(return_expression) = &function.return_expression else {
            return Ok(false);
        };
        // Flatten the guard chain: each level is exactly [If{no else}].
        let mut conditions: Vec<&Expression> = Vec::new();
        let mut body: &[Statement] = &function.statements;
        loop {
            match body {
                [Statement::If { condition, then_body, else_body }] if else_body.is_empty() => {
                    conditions.push(condition);
                    body = then_body;
                }
                _ => break,
            }
        }
        if conditions.is_empty() {
            return Ok(false);
        }
        // A MID-CHAIN early return heads the innermost block (measured:
        // `if ((i0|i1)==0) return 7;` = the record-form test, bne PAST the
        // inline return to the mutations).
        let mut early_return: Option<(&Expression, &Expression)> = None;
        if let [Statement::If { condition, then_body, else_body }, rest @ ..] = body {
            if else_body.is_empty() {
                if let [Statement::Return(Some(value))] = then_body.as_slice() {
                    early_return = Some((condition, value));
                    body = rest;
                }
            }
        }
        // The innermost block: assigns to DISTINCT int params — an i16
        // constant (li), a lis-able constant (measured: 0xbff00000), or a
        // leaf-plus-i16 over a param no EARLIER assign in the block already
        // overwrote (measured: i0 = i1 + 1 before i1's own overwrite).
        enum BlockValue {
            Small(i16),
            High(i16),
            LeafAdd(u8, i16),
            Mask(u8, u8),
        }
        let mut assigns: Vec<(u8, BlockValue)> = Vec::new();
        for statement in body {
            let Statement::Assign { name, value } = statement else {
                return Ok(false);
            };
            let Some(location) = self.locations.get(name.as_str()) else {
                return Ok(false);
            };
            if location.class != ValueClass::General || location.width != 32 {
                return Ok(false);
            }
            let target = location.register;
            let block_value = if let Some(constant) = crate::analysis::constant_value(value) {
                if let Ok(small) = i16::try_from(constant) {
                    BlockValue::Small(small)
                } else if constant & 0xffff == 0 && u32::try_from(constant).is_ok() {
                    BlockValue::High((constant >> 16) as i16)
                } else {
                    return Ok(false);
                }
            } else {
                // Self-masking (`i0 &= C`, desugared): the in-place rlwinm
                // (measured: clrlwi r3,r3,21 in source order).
                if let Expression::Binary { operator: BinaryOperator::BitAnd, left, right } = value {
                    if let Expression::Variable(read) = left.as_ref() {
                        if read == name {
                            if let Some(mask) = crate::analysis::constant_value(right) {
                                if let Some((begin, end)) = crate::analysis::rlwinm_mask(mask) {
                                    if assigns.iter().any(|&(written, _)| written == target) {
                                        return Ok(false);
                                    }
                                    assigns.push((target, BlockValue::Mask(begin, end)));
                                    continue;
                                }
                            }
                        }
                    }
                    return Ok(false);
                }
                // leaf ± i16 (Add with a possibly-negative constant).
                let (leaf, offset) = match value {
                    Expression::Variable(read) => (read, 0i64),
                    Expression::Binary { operator: BinaryOperator::Add, left, right } => {
                        let Expression::Variable(read) = left.as_ref() else {
                            return Ok(false);
                        };
                        let Some(offset) = crate::analysis::constant_value(right) else {
                            return Ok(false);
                        };
                        (read, offset)
                    }
                    Expression::Binary { operator: BinaryOperator::Subtract, left, right } => {
                        let Expression::Variable(read) = left.as_ref() else {
                            return Ok(false);
                        };
                        let Some(offset) = crate::analysis::constant_value(right) else {
                            return Ok(false);
                        };
                        (read, -offset)
                    }
                    _ => return Ok(false),
                };
                let Some(read_location) = self.locations.get(leaf.as_str()) else {
                    return Ok(false);
                };
                if read_location.class != ValueClass::General || read_location.width != 32 {
                    return Ok(false);
                }
                let Ok(offset) = i16::try_from(offset) else {
                    return Ok(false);
                };
                if offset == 0 {
                    // A bare register move inside the block is unmeasured.
                    return Ok(false);
                }
                // The read must precede any overwrite of its register — and
                // a SELF-read (i0 = i0 + 5) reorders in mwcc (the
                // independent li hoists above the self-addi; probed) — defer.
                if read_location.register == target
                    || assigns.iter().any(|&(written, _)| written == read_location.register)
                {
                    return Ok(false);
                }
                BlockValue::LeafAdd(read_location.register, offset)
            };
            if assigns.iter().any(|&(register, _)| register == target) {
                return Ok(false);
            }
            assigns.push((target, block_value));
        }
        if early_return.is_none() && assigns.len() < 2 && conditions.len() < 2 {
            // The single-guard single-assign shapes belong to the measured
            // reassign/select arms.
            return Ok(false);
        }
        if assigns.is_empty() {
            return Ok(false);
        }
        // A bare-variable return folds the guards to conditional RETURNS
        // (bclr) instead of branch-to-join — the reassign arms' territory;
        // this arm takes the expression-return join form only (measured).
        if matches!(return_expression, Expression::Variable(_)) {
            return Ok(false);
        }
        // The return must be claimable by the plain tail evaluator: an
        // expression over params (no calls — gated above).
        // -- commit --
        let join = self.fresh_label();
        for condition in conditions {
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            self.emit_branch_conditional_to(options, condition_bit, join);
        }
        if let Some((condition, value)) = early_return {
            let result = Eabi::general_result().number;
            // A bare return of the value already in r3 FOLDS to a
            // conditional return (measured: or.; beqlr).
            if let Expression::Variable(name) = value {
                if self.lookup_general(name) == Some(result) {
                    let (options, condition_bit) = self.emit_condition_test(condition)?;
                    self.output.instructions.push(Instruction::BranchConditionalToLinkRegister {
                        options: options ^ 8,
                        condition_bit,
                    });
                    for (register, block_value) in &assigns {
                        match block_value {
                            BlockValue::Small(constant) => {
                                self.output.instructions.push(Instruction::load_immediate(*register, *constant));
                            }
                            BlockValue::High(high) => {
                                self.output.instructions.push(Instruction::load_immediate_shifted(*register, *high));
                            }
                            BlockValue::LeafAdd(source, offset) => {
                                self.output.instructions.push(Instruction::AddImmediate {
                                    d: *register,
                                    a: *source,
                                    immediate: *offset,
                                });
                            }
                            BlockValue::Mask(begin, end) => {
                                self.output.instructions.push(Instruction::RotateAndMask {
                                    a: *register,
                                    s: *register,
                                    shift: 0,
                                    begin: *begin,
                                    end: *end,
                                });
                            }
                        }
                    }
                    self.bind_label(join);
                    self.evaluate_tail(return_expression, function.return_type, result)?;
                    self.emit_epilogue_and_return();
                    return Ok(true);
                }
            }
            // Skip the inline return when the early condition fails; the
            // skip lands on the MUTATIONS, not the join.
            let mutations = self.fresh_label();
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            self.emit_branch_conditional_to(options, condition_bit, mutations);
            match crate::analysis::constant_value(value) {
                Some(constant) if i16::try_from(constant).is_ok() => {
                    self.output.instructions.push(Instruction::load_immediate(result, constant as i16));
                }
                Some(_) => return Err(Diagnostic::error("early-return constant beyond i16 (roadmap)")),
                None => {
                    self.evaluate_tail(value, function.return_type, result)?;
                }
            }
            self.emit_epilogue_and_return();
            self.bind_label(mutations);
        }
        for (register, value) in &assigns {
            match value {
                BlockValue::Small(constant) => {
                    self.output.instructions.push(Instruction::load_immediate(*register, *constant));
                }
                BlockValue::High(high) => {
                    self.output.instructions.push(Instruction::load_immediate_shifted(*register, *high));
                }
                BlockValue::LeafAdd(source, offset) => {
                    self.output.instructions.push(Instruction::AddImmediate {
                        d: *register,
                        a: *source,
                        immediate: *offset,
                    });
                }
                BlockValue::Mask(begin, end) => {
                    self.output.instructions.push(Instruction::RotateAndMask {
                        a: *register,
                        s: *register,
                        shift: 0,
                        begin: *begin,
                        end: *end,
                    });
                }
            }
        }
        self.bind_label(join);
        let result = Eabi::general_result().number;
        self.evaluate_tail(return_expression, function.return_type, result)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    pub(crate) fn try_constant_store_if_else(&mut self, function: &Function) -> Compilation<bool> {
        if function_makes_call(function)
            || function.return_type != Type::Void
            || !function.guards.is_empty()
            || !function.locals.is_empty()
        {
            return Ok(false);
        }
        let [Statement::If { condition, then_body, else_body }] = function.statements.as_slice() else {
            return Ok(false);
        };
        // Each arm is handleable when it is a register-valued run or a constant run; pre-check both
        // (no emission) so a non-run arm leaves no orphaned branch.
        let then_plan = self.constant_store_run_plan(then_body);
        let else_plan = self.constant_store_run_plan(else_body);
        let then_registers = self.store_run_arm_registers(then_body);
        let else_registers = self.store_run_arm_registers(else_body);
        if !(then_plan.is_some() || then_registers) || !(else_plan.is_some() || else_registers) {
            return Ok(false);
        }
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let branch_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
        match then_plan {
            Some(plan) => self.emit_constant_store_run(then_body, plan)?,
            None => for statement in then_body { self.emit_statement(statement)?; },
        }
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        let else_label = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
            *target = else_label;
        }
        match else_plan {
            Some(plan) => self.emit_constant_store_run(else_body, plan)?,
            None => for statement in else_body { self.emit_statement(statement)?; },
        }
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// Two computed-value stores to distinct SDA globals (`gi = a+1; gj = b*2;`). mwcc
    /// overlaps the two value computations: it evaluates both first — the earlier into a
    /// real GPR, the later into the scratch `r0` — then stores both (`addi r3,r3,1; slwi
    /// r0,r4,1; stw r3; stw r0`), rather than the unscheduled `compute; store; compute;
    /// store`. We reproduce it by evaluating the first value into a fresh virtual (the
    /// allocator gives it the in-place GPR and keeps it off `r0`, since it is live across
    /// the second computation) and the second into `r0`, then both stores. Leaf/constant
    /// values (no computation to overlap) and 3+ stores fall through to their own paths.
    pub(crate) fn try_computed_store_fill(&mut self, function: &Function) -> Compilation<bool> {
        if function_makes_call(function)
            || function.return_type != Type::Void
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || self.behavior.global_addressing != GlobalAddressing::SmallData
        {
            return Ok(false);
        }
        // The two-store run is either the whole body, or the body of a single trailing `if (c) { … }`
        // with no else — the same latency-scheduled value overlap, wrapped in a conditional return.
        // Detection is emission-free, so the guard is emitted just before the value evaluations.
        let (statements, guard): (&[Statement], Option<&Expression>) = match function.statements.as_slice() {
            [Statement::If { condition, then_body, else_body }] if else_body.is_empty() => (then_body.as_slice(), Some(condition)),
            other => (other, None),
        };
        if statements.len() != 2 {
            return Ok(false);
        }
        // Both statements must store to a distinct SDA global. Each value is a single-op
        // computation or a constant; a bare register leaf needs no overlap and goes through
        // try_mixed_store_fill / the normal path.
        let mut stores = Vec::new();
        for statement in statements {
            let Statement::Store { target, value } = statement else { return Ok(false) };
            let Expression::Variable(name) = target else { return Ok(false) };
            let Some(&global_type) = self.globals.get(name.as_str()) else { return Ok(false) };
            // Integer globals only — this path evaluates values through the general
            // (integer) evaluator; a float global/value goes through the float path.
            if matches!(global_type, Type::Float | Type::Double) {
                return Ok(false);
            }
            let Some(pointee) = pointee_of_type(global_type) else { return Ok(false) };
            // A single-instruction op over register operands, or a constant (materialized
            // with `li`, ordered as a low-latency value) — both shapes this path can
            // schedule. A memory read, comparison idiom, modulo, or nested value reorders
            // or needs more, and a bare register leaf goes through try_mixed_store_fill.
            if !self.is_single_op_register_value(value) && constant_value(value).is_none() {
                return Ok(false);
            }
            stores.push((name.clone(), pointee, value.clone()));
        }
        // At least one value must be a genuine computation. Two constants are the constant
        // fill's domain (it dedups a repeated value to one `li`); this overlap path would
        // emit a separate `li` per store.
        if !self.is_single_op_register_value(&stores[0].2) && !self.is_single_op_register_value(&stores[1].2) {
            return Ok(false);
        }
        if stores[0].0 == stores[1].0 {
            // Same target: the first store is dead (mwcc emits only the second) — a
            // dead-store elimination this path does not model, so defer.
            return Ok(false);
        }
        // The first store's value lives in a virtual (the allocator gives it the in-place
        // GPR, off r0 since it is live across the other op), the second in the scratch r0.
        // mwcc issues the heavier op first and stores the quicker value first, so order the
        // two evaluations and the two stores by latency.
        let high = [self.value_latency_is_high(&stores[0].2), self.value_latency_is_high(&stores[1].2)];
        // Evaluate the heavier value first so the allocator can reuse a spent operand
        // register for the lighter one. Weight: high-latency op > single-cycle op >
        // constant — a constant is just an `li`, materialized last once the computation has
        // freed its operand register (`gi=5; gj=a+1` → `addi r0,r3,1; li r3,5`, the `5`
        // reusing a's register).
        let weight = |is_high: bool, is_constant: bool| -> u8 {
            if is_high { 2 } else if is_constant { 0 } else { 1 }
        };
        let weights = [
            weight(high[0], constant_value(&stores[0].2).is_some()),
            weight(high[1], constant_value(&stores[1].2).is_some()),
        ];
        // For the trailing-if form, the conditional return precedes the value overlap
        // (`cmpwi;beqlr; <two values>; <two stores>`).
        if let Some(condition) = guard {
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
        }
        let first_register = self.fresh_virtual_general();
        if weights[1] > weights[0] {
            // The second value is the heavier op: compute it (into r0) first.
            self.evaluate_general(&stores[1].2, GENERAL_SCRATCH)?;
            self.evaluate_general(&stores[0].2, first_register)?;
        } else {
            self.evaluate_general(&stores[0].2, first_register)?;
            self.evaluate_general(&stores[1].2, GENERAL_SCRATCH)?;
        }
        if high[0] && !high[1] {
            // The first value is the long op: store the quicker second value first.
            self.emit_sda_global_store_from(&stores[1].0, stores[1].1, GENERAL_SCRATCH);
            self.emit_sda_global_store_from(&stores[0].0, stores[0].1, first_register);
        } else {
            self.emit_sda_global_store_from(&stores[0].0, stores[0].1, first_register);
            self.emit_sda_global_store_from(&stores[1].0, stores[1].1, GENERAL_SCRATCH);
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// Two stores to distinct integer SDA globals where one value is a register-resident
    /// leaf parameter and the other a "filler" — a single-op computation (`gi=a+1; gj=b;`)
    /// or a constant (`gi=a; gj=5;`). mwcc produces the filler into the scratch, then stores
    /// the LEAF first (it is ready immediately) and the filler second — `addi r0,r3,1; stw
    /// r4,gj; stw r0,gi` or `li r0,5; stw r3,gi; stw r0,gj`. (Both-computed and computed+
    /// constant are the latency-ordered fill above; both-leaf is the normal path; both-
    /// constant is the constant fill.)
    pub(crate) fn try_mixed_store_fill(&mut self, function: &Function) -> Compilation<bool> {
        if function_makes_call(function)
            || function.return_type != Type::Void
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || self.behavior.global_addressing != GlobalAddressing::SmallData
        {
            return Ok(false);
        }
        // Either the whole body, or a single trailing `if (c) { … }` (no else) — the same
        // leaf/filler pairing, wrapped in a conditional return emitted just before the filler.
        let (statements, guard): (&[Statement], Option<&Expression>) = match function.statements.as_slice() {
            [Statement::If { condition, then_body, else_body }] if else_body.is_empty() => (then_body.as_slice(), Some(condition)),
            other => (other, None),
        };
        if statements.len() != 2 {
            return Ok(false);
        }
        let mut stores = Vec::new();
        for statement in statements {
            let Statement::Store { target: Expression::Variable(name), value } = statement else { return Ok(false) };
            let Some(&global_type) = self.globals.get(name.as_str()) else { return Ok(false) };
            if matches!(global_type, Type::Float | Type::Double) {
                return Ok(false);
            }
            let Some(pointee) = pointee_of_type(global_type) else { return Ok(false) };
            stores.push((name.clone(), pointee, value.clone()));
        }
        if stores[0].0 == stores[1].0 {
            return Ok(false);
        }
        // Exactly one value is a "filler" — a single-op computation or a constant — and the
        // other a register-resident leaf parameter (a global/memory leaf would need a load).
        // The filler is materialized into the scratch and the leaf stays in its register, so
        // both `gi=a+1; gj=b;` and `gi=a; gj=5;` reduce to: produce the filler, store the
        // leaf, store the filler.
        let filler = [
            self.is_single_op_register_value(&stores[0].2) || constant_value(&stores[0].2).is_some(),
            self.is_single_op_register_value(&stores[1].2) || constant_value(&stores[1].2).is_some(),
        ];
        let is_register_leaf = |value: &Expression| {
            matches!(value, Expression::Variable(name) if !self.globals.contains_key(name.as_str()))
        };
        let (filler, leaf) = if filler[0] && is_register_leaf(&stores[1].2) {
            (0usize, 1usize)
        } else if is_register_leaf(&stores[0].2) && filler[1] {
            (1usize, 0usize)
        } else {
            return Ok(false);
        };
        // The filler goes into the scratch; the leaf is already in its register, so store it
        // first, then the filler. For the trailing-if form the conditional return precedes them.
        let leaf_register = self.general_register_of_leaf(&stores[leaf].2)?;
        if let Some(condition) = guard {
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
        }
        self.evaluate_general(&stores[filler].2, GENERAL_SCRATCH)?;
        self.emit_sda_global_store_from(&stores[leaf].0, stores[leaf].1, leaf_register);
        self.emit_sda_global_store_from(&stores[filler].0, stores[filler].1, GENERAL_SCRATCH);
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// Three or more stores to distinct integer SDA globals where exactly one value is a
    /// constant and the rest are register-resident leaf parameters (`gi=a; gj=b; gk=5;`).
    /// mwcc hoists the constant's `li` into the scratch up front and stores in source order
    /// — except a constant store cannot occupy the `li`'s one-cycle latency slot, so if the
    /// constant is the FIRST store it swaps with the next (leaf) store:
    ///
    ///     gi=a; gj=b; gk=5  ->  li r0,5; stw r3,gi; stw r4,gj; stw r0,gk   (source order)
    ///     gi=5; gj=a; gk=b  ->  li r0,5; stw r3,gj; stw r0,gi; stw r4,gk   (leading const swaps)
    ///
    /// (Two stores are the mixed fill; all-constant is the constant fill; a non-leaf, non-
    /// constant value among the rest needs the general scheduler and defers.)
    pub(crate) fn try_leaf_constant_fill(&mut self, function: &Function) -> Compilation<bool> {
        if function_makes_call(function)
            || function.return_type != Type::Void
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || function.statements.len() < 3
            || self.behavior.global_addressing != GlobalAddressing::SmallData
        {
            return Ok(false);
        }
        let mut stores = Vec::new();
        for statement in &function.statements {
            let Statement::Store { target: Expression::Variable(name), value } = statement else { return Ok(false) };
            let Some(&global_type) = self.globals.get(name.as_str()) else { return Ok(false) };
            if matches!(global_type, Type::Float | Type::Double) {
                return Ok(false);
            }
            let Some(pointee) = pointee_of_type(global_type) else { return Ok(false) };
            stores.push((name.clone(), pointee, value.clone()));
        }
        // Distinct targets (a repeated target is a dead store this path does not model).
        for outer in 0..stores.len() {
            for inner in (outer + 1)..stores.len() {
                if stores[outer].0 == stores[inner].0 {
                    return Ok(false);
                }
            }
        }
        // Exactly one constant; every other value a register-resident leaf parameter.
        let mut constant_index = None;
        for (index, store) in stores.iter().enumerate() {
            if constant_value(&store.2).is_some() {
                if constant_index.is_some() {
                    return Ok(false);
                }
                constant_index = Some(index);
            } else if !matches!(&store.2, Expression::Variable(name) if !self.globals.contains_key(name.as_str())) {
                return Ok(false);
            }
        }
        let Some(constant_index) = constant_index else {
            return Ok(false);
        };
        let constant = constant_value(&stores[constant_index].2).unwrap();
        self.load_integer_constant(GENERAL_SCRATCH, constant as i64);
        // Source order, except a leading constant store swaps with the next store so it does
        // not sit in the `li`'s latency slot.
        let mut order: Vec<usize> = (0..stores.len()).collect();
        if constant_index == 0 {
            order.swap(0, 1);
        }
        for &index in &order {
            if index == constant_index {
                self.emit_sda_global_store_from(&stores[index].0, stores[index].1, GENERAL_SCRATCH);
            } else {
                let register = self.general_register_of_leaf(&stores[index].2)?;
                self.emit_sda_global_store_from(&stores[index].0, stores[index].1, register);
            }
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// Whether `value` is a single-instruction arithmetic op over register-resident
    /// operands (parameters and constants) — the shape the computed-store-fill schedules.
    /// Includes the single-cycle ops (add/sub/and/or/xor/shift/neg/not, power-of-two
    /// multiply) and the multi-cycle but single-instruction ops (register/immediate
    /// multiply, divide), whose latency the fill orders around. Excluded (need more than a
    /// register-shuffle): modulo and comparisons (multi-instruction idioms), a nested
    /// value (needs an intermediate), and a memory read (needs load hoisting).
    pub(crate) fn is_single_op_register_value(&self, value: &Expression) -> bool {
        let is_register_leaf = |operand: &Expression| match operand {
            // A NARROW (char/short) register is not a single-op leaf: it needs a
            // re-extension first (extsb/extsh/clrlwi), whose latency reorders the
            // scheduled overlap — those shapes go through the DAG emitter.
            Expression::Variable(name) => {
                !self.globals.contains_key(name.as_str())
                    && self.locations.get(name.as_str()).is_none_or(|location| location.width == 32)
            }
            Expression::IntegerLiteral(_) | Expression::FloatLiteral(_) => true,
            _ => false,
        };
        match value {
            Expression::Binary { operator, left, right } => {
                is_register_leaf(left)
                    && is_register_leaf(right)
                    && matches!(
                        operator,
                        BinaryOperator::Add | BinaryOperator::Subtract
                            | BinaryOperator::BitAnd | BinaryOperator::BitOr | BinaryOperator::BitXor
                            | BinaryOperator::ShiftLeft | BinaryOperator::ShiftRight
                            | BinaryOperator::Multiply | BinaryOperator::Divide
                    )
            }
            Expression::Unary { operator: UnaryOperator::Negate | UnaryOperator::BitNot, operand } => is_register_leaf(operand),
            _ => false,
        }
    }

    /// Whether a single-op value is multi-cycle (a register or non-power-of-two multiply,
    /// or a divide) rather than single-cycle. mwcc issues the long op early and stores the
    /// quick value first; the computed-store-fill orders the two values by this.
    fn value_latency_is_high(&self, value: &Expression) -> bool {
        let is_power_of_two = |operand: &Expression| {
            matches!(operand, Expression::IntegerLiteral(n) if *n > 0 && (*n & (*n - 1)) == 0)
        };
        match value {
            Expression::Binary { operator: BinaryOperator::Multiply, left, right } => {
                !(is_power_of_two(left) || is_power_of_two(right))
            }
            Expression::Binary { operator: BinaryOperator::Divide, .. } => true,
            _ => false,
        }
    }

    /// Whether a store to `target` writes only memory (and the value register),
    /// never the scratch — so a constant-fill run can keep its value live in the
    /// scratch across it. A leaf-based member/dereference/constant-index store
    /// qualifies; a global (absolute-addressing base) or variable index (scratch
    /// scaling) does not.
    fn is_scratch_safe_store_target(&self, target: &Expression) -> bool {
        match target {
            Expression::Member { base, .. } => matches!(base.as_ref(), Expression::Variable(_)),
            Expression::Dereference { pointer } => matches!(pointer.as_ref(), Expression::Variable(_)),
            Expression::Index { base, index } => {
                matches!(base.as_ref(), Expression::Variable(_)) && constant_value(index).is_some()
            }
            // A small-data (SDA21) integer global store folds the relocation into the
            // store itself (`stw r0, g@sda21`) — no base register, and it never writes the
            // scratch — so a constant fill can keep its value live across it. An absolute-
            // addressing global needs a base register, so it stays excluded.
            Expression::Variable(name) => {
                matches!(self.behavior.global_addressing, GlobalAddressing::SmallData)
                    && self.globals.get(name.as_str()).is_some_and(|global_type| !matches!(global_type, Type::Float | Type::Double))
            }
            _ => false,
        }
    }

    /// The register holding the base pointer of a scratch-safe store target.
    fn store_base_register(&self, target: &Expression) -> Option<u8> {
        let name = match target {
            Expression::Member { base, .. } | Expression::Index { base, .. } => leaf_name(base),
            Expression::Dereference { pointer } => leaf_name(pointer),
            _ => None,
        }?;
        self.lookup_general(name)
    }

    /// Tear down the stack frame (if one was allocated) and return. A non-leaf
    /// function restores the link register from `frame_size + 4` first.
    /// A `void` function whose whole body is `do { …calls… } while (--counter);`
    /// with the counter a parameter: mwcc keeps the counter in a callee-saved
    /// register (r31), runs the body, then `addic. r31,r31,-1` (decrement, set CR0)
    /// and `bne` back to the loop top. Returns whether this path applied.
    fn try_do_while_counter(&mut self, function: &Function) -> Compilation<bool> {
        if !function.guards.is_empty() || !function.locals.is_empty() || !self.frame_slots.is_empty() {
            return Ok(false);
        }
        if function.return_type != Type::Void {
            return Ok(false);
        }
        let [Statement::Loop { kind, initializer: None, condition: Some(condition), step: None, body }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        let kind = *kind;
        if !matches!(kind, LoopKind::DoWhile | LoopKind::While) {
            return Ok(false);
        }
        // The condition must be `--counter` (a parameter decrement), which the
        // parser lowered to `counter = counter - 1`.
        let counter = match condition {
            Expression::Assign { target, value } => match (target.as_ref(), value.as_ref()) {
                (
                    Expression::Variable(name),
                    Expression::Binary { operator: BinaryOperator::Subtract, left, right },
                ) if matches!(left.as_ref(), Expression::Variable(other) if other == name)
                    && matches!(right.as_ref(), Expression::IntegerLiteral(1)) =>
                {
                    name.clone()
                }
                _ => return Ok(false),
            },
            _ => return Ok(false),
        };
        if !function.parameters.iter().any(|parameter| parameter.name == counter) {
            return Ok(false);
        }
        // The body must be straight-line calls that do not pass the counter as an
        // argument (the first such use would stay in the incoming register — the
        // value-location nuance the callee-saved path also defers).
        if body.iter().any(|statement| !matches!(statement, Statement::Expression(_))) {
            return Ok(false);
        }
        if body.iter().any(|statement| matches!(statement, Statement::Expression(e) if expression_reads_name(e, &counter))) {
            return Ok(false);
        }
        if !function_makes_call(function) {
            return Ok(false);
        }
        let (class, incoming) = match self.locations.get(&counter) {
            Some(location) => (location.class, location.register),
            None => return Ok(false),
        };
        if class != ValueClass::General {
            return Ok(false);
        }

        // Prologue: save the link register and r31, move the counter into r31.
        const SAVED: u8 = 31;
        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved = vec![SAVED];
        // The loop's internal labels advance mwcc's anonymous-`@N` counter — by 6
        // for a do-while, by 4 for a while (the extra top branch, fewer labels).
        self.output.anonymous_label_bump = if matches!(kind, LoopKind::DoWhile) { 6 } else { 4 };
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: SAVED, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::Or { a: SAVED, s: incoming, b: incoming });
        if let Some(location) = self.locations.get_mut(&counter) {
            location.register = SAVED;
        }

        // A while loop tests the condition first: branch down to the
        // decrement-and-test, which falls through into the body on re-entry.
        let skip_to_condition = if matches!(kind, LoopKind::While) {
            let index = self.output.instructions.len();
            self.output.instructions.push(Instruction::Branch { target: 0 });
            Some(index)
        } else {
            None
        };
        // The loop body, then the decrement-and-test and the backward branch.
        let body_top = self.output.instructions.len();
        for statement in body {
            self.emit_statement(statement)?;
        }
        if let Some(index) = skip_to_condition {
            let condition = self.output.instructions.len();
            if let Instruction::Branch { target } = &mut self.output.instructions[index] {
                *target = condition;
            }
        }
        self.output.instructions.push(Instruction::AddImmediateCarryingRecord { d: SAVED, a: SAVED, immediate: -1 });
        // `bne body_top`: branch when CR0[EQ] is clear (BO=4, BI=2). Backward, which
        // the encoder resolves from the instruction indices.
        self.output.instructions.push(Instruction::BranchConditionalForward { options: 4, condition_bit: 2, target: body_top });

        // Epilogue, emitted in final order (the loop's branch makes the scheduler and
        // the LR-reload hoist bail): the LR reload comes before the r31 reload.
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: self.frame_size + 4 });
        self.output.instructions.push(Instruction::LoadWord { d: SAVED, a: 1, offset: self.frame_size - 4 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: self.frame_size });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// A leaf `void` function whose body is a single non-counting `while` loop (truthy condition)
    /// whose body is pure in-place increments/decrements of register parameters (`while (*p) p++;`).
    /// mwcc does
    /// NOT unroll these; it emits the rotated form `[b COND;] BODY: <addi>; COND: <test>; <bne BODY>;
    /// blr` with no frame (leaf). The body is emitted directly via `evaluate_general` into each
    /// variable's OWN register, so the loop-carried mutation stays in place rather than being
    /// value-tracked across the back-edge (the linear value tracker has no back-edge). A store in the
    /// body (mwcc hoists loop-invariant store values — not modeled), an empty body (different
    /// structure: the condition is the loop top), a call, a local, a guard, a counted loop (mwcc
    /// unrolls), or a non-void return falls through to defer.
    fn try_emit_increment_while(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::Void
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || !function.locals.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        let [Statement::Loop { kind, initializer: None, condition: Some(condition), step: None, body }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        let kind = *kind;
        // A do-while (mwcc fuses the body increment with the next condition load into `lwzu`) or a
        // comparison condition (`while (p < e)` — mwcc computes the trip count and emits a counted
        // CTR loop, `mtctr`/`bdnz`) is deferred; only a `while` with a truthy condition keeps the
        // plain rotated form this models.
        if !matches!(kind, LoopKind::While) || body.is_empty() {
            return Ok(false);
        }
        // A comparison of the loop counter against a loop-invariant bound (`while (p < e)`) lets mwcc
        // compute the trip count and emit a counted CTR loop, so it is deferred. But a comparison of a
        // DATA-DEPENDENT value (`while (*p != 0)`, `while (*p > 0)`) has no computable trip count and
        // stays the rotated form — allow it when one side is a dereference and the other a constant.
        if let Expression::Binary { operator, left, right } = condition {
            if crate::analysis::is_comparison(*operator) {
                let dereference_vs_constant = (matches!(left.as_ref(), Expression::Dereference { .. }) && matches!(right.as_ref(), Expression::IntegerLiteral(_)))
                    || (matches!(right.as_ref(), Expression::Dereference { .. }) && matches!(left.as_ref(), Expression::IntegerLiteral(_)));
                if !dereference_vs_constant {
                    return Ok(false);
                }
            }
        }
        // Every body statement must be an in-place `var = var +/- const` on a register parameter — the
        // increment/decrement of a scan pointer or index. No stores, calls, loads, or nested control.
        for statement in body {
            let Statement::Assign { name, value } = statement else {
                return Ok(false);
            };
            if self.lookup_general(name).is_none() {
                return Ok(false);
            }
            // The incremented variable must be a POINTER: mwcc countifies an integer increment loop
            // (`while (x) x++;` -> neg/mtctr/bdnz, trip count `-x`) but leaves a pointer scan as the
            // rotated form this models.
            if self.locations.get(name).map_or(true, |location| location.pointee.is_none()) {
                return Ok(false);
            }
            let is_increment = matches!(value, Expression::Binary { operator: BinaryOperator::Add | BinaryOperator::Subtract, left, right }
                if matches!(left.as_ref(), Expression::Variable(other) if other == name)
                    && matches!(right.as_ref(), Expression::IntegerLiteral(_)));
            if !is_increment {
                return Ok(false);
            }
        }
        // The loop's labels advance mwcc's anonymous-`@N` counter (4 for a while, 6 for a do-while).
        self.output.anonymous_label_bump = if matches!(kind, LoopKind::DoWhile) { 6 } else { 4 };
        // A while tests the condition first: branch down to the test; a do-while falls into the body.
        let skip = if matches!(kind, LoopKind::While) {
            let index = self.output.instructions.len();
            self.output.instructions.push(Instruction::Branch { target: 0 });
            Some(index)
        } else {
            None
        };
        let body_top = self.output.instructions.len();
        for statement in body {
            if let Statement::Assign { name, value } = statement {
                let register = self.lookup_general(name).expect("gated to a register variable above");
                self.evaluate_general(value, register)?;
            }
        }
        if let Some(index) = skip {
            let condition_at = self.output.instructions.len();
            if let Instruction::Branch { target } = &mut self.output.instructions[index] {
                *target = condition_at;
            }
        }
        // emit_condition_test gives the body-SKIP branch (taken when the condition is FALSE); the loop
        // branches BACK to the body when it is TRUE, so invert the BO (branch-if-clear <-> -if-set).
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let back = if options == 4 { 12 } else { 4 };
        self.output.instructions.push(Instruction::BranchConditionalForward { options: back, condition_bit, target: body_top });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// A `void` function whose body is a counting `for (i = 0; i < bound; i++)`
    /// loop with a parameter bound: mwcc puts the counter in r31 (callee-saved,
    /// initialised to 0) and the bound in r30, branches to the test, and runs
    /// `BODY: <body>; addi r31,r31,1; cmpw r31,r30; blt BODY`. The body may use the
    /// counter (passed as a call argument). Returns whether this path applied.
    fn try_for_counter(&mut self, function: &Function) -> Compilation<bool> {
        if !function.guards.is_empty() || !self.frame_slots.is_empty() || function.return_type != Type::Void {
            return Ok(false);
        }
        let [Statement::Loop { kind: LoopKind::For, initializer: Some(init), condition: Some(condition), step: Some(step), body }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        // `i = 0`.
        let counter = match init {
            Expression::Assign { target, value } if matches!(value.as_ref(), Expression::IntegerLiteral(0)) => {
                match target.as_ref() {
                    Expression::Variable(name) => name.clone(),
                    _ => return Ok(false),
                }
            }
            _ => return Ok(false),
        };
        // `i < bound`.
        let bound = match condition {
            Expression::Binary { operator: BinaryOperator::Less, left, right }
                if matches!(left.as_ref(), Expression::Variable(name) if *name == counter) =>
            {
                match right.as_ref() {
                    Expression::Variable(name) => name.clone(),
                    _ => return Ok(false),
                }
            }
            _ => return Ok(false),
        };
        // `i = i + 1`.
        let step_increments = matches!(step, Expression::Assign { target, value }
            if matches!(target.as_ref(), Expression::Variable(name) if *name == counter)
            && matches!(value.as_ref(), Expression::Binary { operator: BinaryOperator::Add, left, right }
                if matches!(left.as_ref(), Expression::Variable(name) if *name == counter)
                && matches!(right.as_ref(), Expression::IntegerLiteral(1))));
        if !step_increments {
            return Ok(false);
        }
        // The counter is the function's only local (uninitialised — the for-clause
        // sets it); the bound is a parameter.
        let [local] = function.locals.as_slice() else {
            return Ok(false);
        };
        if local.name != counter || local.initializer.is_some() {
            return Ok(false);
        }
        if !function.parameters.iter().any(|parameter| parameter.name == bound) {
            return Ok(false);
        }
        // The body must be straight-line calls referencing no register value other
        // than the counter (the bound, and any other parameter, would each need
        // their own callee-saved register — deferred).
        if body.iter().any(|statement| !matches!(statement, Statement::Expression(_))) {
            return Ok(false);
        }
        let reads_other_parameter = body.iter().any(|statement| match statement {
            Statement::Expression(expression) => function
                .parameters
                .iter()
                .any(|parameter| parameter.name != counter && expression_reads_name(expression, &parameter.name)),
            _ => false,
        });
        if reads_other_parameter {
            return Ok(false);
        }
        if !function_makes_call(function) {
            return Ok(false);
        }
        let bound_incoming = match self.locations.get(&bound) {
            Some(location) if location.class == ValueClass::General => location.register,
            _ => return Ok(false),
        };

        // Prologue: r31 = counter (init 0), r30 = bound, saved at the top of a frame.
        const COUNTER: u8 = 31;
        const BOUND: u8 = 30;
        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved = vec![COUNTER, BOUND];
        self.output.anonymous_label_bump = 5;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreWord { s: COUNTER, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::AddImmediate { d: COUNTER, a: 0, immediate: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: BOUND, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::Or { a: BOUND, s: bound_incoming, b: bound_incoming });
        let signed = self.signed_of(local.declared_type);
        self.locations.insert(
            counter.clone(),
            Location { class: ValueClass::General, register: COUNTER, signed, width: 32, pointee: None, stride: None },
        );
        if let Some(location) = self.locations.get_mut(&bound) {
            location.register = BOUND;
        }

        // Branch to the test; the body falls into the step, then the compare loops.
        let skip = self.output.instructions.len();
        self.output.instructions.push(Instruction::Branch { target: 0 });
        let body_top = self.output.instructions.len();
        for statement in body {
            self.emit_statement(statement)?;
        }
        self.output.instructions.push(Instruction::AddImmediate { d: COUNTER, a: COUNTER, immediate: 1 });
        let condition_index = self.output.instructions.len();
        if let Instruction::Branch { target } = &mut self.output.instructions[skip] {
            *target = condition_index;
        }
        self.output.instructions.push(Instruction::CompareWord { a: COUNTER, b: BOUND });
        // `blt body_top` (BO=12 branch-if-true, BI=0 the LT bit).
        self.output.instructions.push(Instruction::BranchConditionalForward { options: 12, condition_bit: 0, target: body_top });

        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::LoadWord { d: COUNTER, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::LoadWord { d: BOUND, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// A straight-line non-leaf function whose parameters live across its call(s):
    /// mwcc copies each into a callee-saved register at entry (saved/reloaded around
    /// the frame) so it survives the calls. The registers are assigned by parameter
    /// order — the LAST live parameter gets r31, the next r30, and so on — and the
    /// body/return then read the values from those registers. Returns whether it
    /// applied. (Locals, floats, and values passed to a call still defer.)
    /// The PIPELINED COPY (fire 417, the strcpy idiom): `char *p = dst;
    /// while ((*p++ = *src++)) ;` — the assignment IS the condition, so
    /// there is no separate test block. Measured: mr alias; LOOP: lbz
    /// carry,0(src); addi src,1; extsb. (the test); stb carry,0(p);
    /// addi p,1; bne LOOP; blr — the alias takes params_top+2 (r6) and
    /// the carried char params_top+1 (r5); dst rides r3 to the return.
    fn try_pipelined_copy(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::{LoopKind, Statement};
        if function.return_type != Type::Pointer(Pointee::Char)
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        let [dst_param, src_param] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if dst_param.parameter_type != Type::Pointer(Pointee::Char)
            || src_param.parameter_type != Type::Pointer(Pointee::Char)
        {
            return Ok(false);
        }
        let dst = dst_param.name.as_str();
        let source = src_param.name.as_str();
        let [alias_local] = function.locals.as_slice() else {
            return Ok(false);
        };
        if alias_local.declared_type != Type::Pointer(Pointee::Char)
            || !matches!(&alias_local.initializer, Some(Expression::Variable(v)) if v == dst)
        {
            return Ok(false);
        }
        let alias = alias_local.name.as_str();
        let [Statement::Loop { kind: LoopKind::While, initializer: None, condition: Some(condition), step: None, body }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        if !body.is_empty() {
            return Ok(false);
        }
        // The condition: *p++ = *src++ (both POSTFIX — the old pointers).
        let post_deref = |expression: &Expression| -> Option<String> {
            let Expression::Dereference { pointer } = expression else { return None };
            let Expression::PostStep { target, operator: BinaryOperator::Add } = pointer.as_ref()
            else {
                return None;
            };
            let Expression::Variable(name) = target.as_ref() else { return None };
            Some(name.clone())
        };
        let Expression::Assign { target, value } = condition else {
            return Ok(false);
        };
        if post_deref(target).as_deref() != Some(alias) || post_deref(value).as_deref() != Some(source)
        {
            return Ok(false);
        }
        if !matches!(&function.return_expression, Some(Expression::Variable(v)) if v == dst) {
            return Ok(false);
        }
        let Some(dst_register) = self.lookup_general(dst) else {
            return Ok(false);
        };
        let Some(src_register) = self.lookup_general(source) else {
            return Ok(false);
        };
        let top = dst_register.max(src_register);
        let carry = top + 1;
        let alias_register = top + 2;
        // -- emit --
        self.output.instructions.push(Instruction::move_register(alias_register, dst_register));
        let loop_at = self.fresh_label();
        self.bind_label(loop_at);
        self.output.instructions.push(Instruction::LoadByteZero { d: carry, a: src_register, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: src_register, a: src_register, immediate: 1 });
        self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: carry });
        self.output.instructions.push(Instruction::StoreByte { s: carry, a: alias_register, offset: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: alias_register, a: alias_register, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, loop_at); // bne
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // @N: measured after implementation (objprobe) — placeholder 0.
        self.output.anonymous_label_bump += 0;
        Ok(true)
    }

    /// The CTR LOOP (fire 419, e_fmod's `while(n--)` walker): a counted
    /// loop whose body BRANCHES escapes the ×8 unroll entirely — mwcc
    /// emits `mtctr n; cmpwi n,0; beq(lr); BODY; bdnz BODY`. The skip
    /// branch mirrors the entry test exactly: `while(n--)` skips only on
    /// n==0 (a negative n runs 2^32 times, and the unsigned CTR does
    /// too — faithful). Captured micro-shape: `hz = hx - K` fuses into
    /// `addic. r0` (the condition-only computed rides r0 through the
    /// arm), the diamond writes the param home directly in both arms,
    /// and post-loop code takes `beq END` instead of `beqlr`. The
    /// `for(i<n)` variant if-converts its diamond differently (eager
    /// else + `mr` join) and is NOT claimed here; straight-line bodies
    /// take the ×8 unroll machinery (deferred, the counted gate).
    fn try_ctr_loop(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::{LoopKind, Statement};
        if function.return_type != Type::Int
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        let [Statement::Loop { kind: LoopKind::While, initializer: None, condition: Some(condition), step: None, body }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        // The condition: a bare `n--` of an int parameter.
        let Expression::PostStep { target, operator: BinaryOperator::Subtract } = condition else {
            return Ok(false);
        };
        let Expression::Variable(count) = target.as_ref() else {
            return Ok(false);
        };
        if !function
            .parameters
            .iter()
            .any(|parameter| parameter.name == *count && parameter.parameter_type == Type::Int)
        {
            return Ok(false);
        }
        // The body: `hz = hx - K; if (hz < 0) hx = hx + hx; else hx = hz + hz;`
        let [Statement::Assign { name: hz, value: hz_value }, Statement::If { condition: test, then_body, else_body }] =
            body.as_slice()
        else {
            return Ok(false);
        };
        let Expression::Binary { operator: BinaryOperator::Subtract, left, right } = hz_value else {
            return Ok(false);
        };
        let Expression::Variable(hx) = left.as_ref() else {
            return Ok(false);
        };
        // The head: `hx - K` folds into `addic. r0` (fire 419); `hx - hy`
        // (hy an int parameter) into `subf. r0, hy, hx` (fire 420).
        enum Head {
            Immediate(i16),
            Register(String),
        }
        let head = match right.as_ref() {
            Expression::IntegerLiteral(k) => {
                let Ok(negated_k) = i16::try_from(-*k) else {
                    return Ok(false);
                };
                Head::Immediate(negated_k)
            }
            Expression::Variable(hy) if hy != hx && hy != hz && hy != count => {
                if !function
                    .parameters
                    .iter()
                    .any(|parameter| parameter.name == *hy && parameter.parameter_type == Type::Int)
                {
                    return Ok(false);
                }
                Head::Register(hy.clone())
            }
            _ => return Ok(false),
        };
        if hx == hz || hx == count || hz == count {
            return Ok(false);
        }
        // hx: an int parameter sitting in r3 (every capture); hz: an int
        // local or a dead-on-entry int parameter (its home is never
        // touched — the value rides r0).
        if !function
            .parameters
            .iter()
            .any(|parameter| parameter.name == *hx && parameter.parameter_type == Type::Int)
        {
            return Ok(false);
        }
        let hz_is_local = function
            .locals
            .iter()
            .any(|local| local.name == *hz && local.declared_type == Type::Int && local.initializer.is_none());
        let hz_is_dead_parameter = function
            .parameters
            .iter()
            .any(|parameter| parameter.name == *hz && parameter.parameter_type == Type::Int);
        if !hz_is_local && !hz_is_dead_parameter {
            return Ok(false);
        }
        let Expression::Binary { operator: BinaryOperator::Less, left: test_left, right: test_right } = test
        else {
            return Ok(false);
        };
        if !matches!(test_left.as_ref(), Expression::Variable(v) if v == hz)
            || !matches!(test_right.as_ref(), Expression::IntegerLiteral(0))
        {
            return Ok(false);
        }
        let doubles_into = |statement: &Statement, target: &str, doubled: &str| -> bool {
            let Statement::Assign { name, value } = statement else {
                return false;
            };
            if name != target {
                return false;
            }
            let Expression::Binary { operator: BinaryOperator::Add, left, right } = value else {
                return false;
            };
            matches!(left.as_ref(), Expression::Variable(v) if v == doubled)
                && matches!(right.as_ref(), Expression::Variable(v) if v == doubled)
        };
        // The then arm: `hx = hx + hx;` (double, fire 419) or the PAIR
        // CARRY STEP `hx = hx + hx + (lx >> 31); lx = lx + lx;` (fire
        // 420, e_fmod's 2-word left shift — the srwi leads, the LOW
        // doubling schedules between it and the two adds, which
        // associate hx + (hx + carry)). The low word must be UNSIGNED
        // (a signed one would srawi).
        enum ThenArm {
            Double,
            PairStep(String),
        }
        let then_arm = match then_body.as_slice() {
            [single] if doubles_into(single, hx, hx) => ThenArm::Double,
            [Statement::Assign { name: high_name, value: high_value }, low_step] => {
                if high_name != hx {
                    return Ok(false);
                }
                let Expression::Binary { operator: BinaryOperator::Add, left: sum, right: carry } =
                    high_value
                else {
                    return Ok(false);
                };
                let Expression::Binary { operator: BinaryOperator::Add, left: first, right: second } =
                    sum.as_ref()
                else {
                    return Ok(false);
                };
                if !matches!(first.as_ref(), Expression::Variable(v) if v == hx)
                    || !matches!(second.as_ref(), Expression::Variable(v) if v == hx)
                {
                    return Ok(false);
                }
                let Expression::Binary { operator: BinaryOperator::ShiftRight, left: low, right: amount } =
                    carry.as_ref()
                else {
                    return Ok(false);
                };
                let Expression::Variable(lx) = low.as_ref() else {
                    return Ok(false);
                };
                if !matches!(amount.as_ref(), Expression::IntegerLiteral(31))
                    || lx == hx
                    || lx == hz
                    || lx == count
                    || matches!(&head, Head::Register(hy) if lx == hy)
                    || !doubles_into(low_step, lx, lx)
                    || !function
                        .parameters
                        .iter()
                        .any(|parameter| parameter.name == *lx && parameter.parameter_type == Type::UnsignedInt)
                {
                    return Ok(false);
                }
                ThenArm::PairStep(lx.clone())
            }
            _ => return Ok(false),
        };
        let [else_single] = else_body.as_slice() else {
            return Ok(false);
        };
        if !doubles_into(else_single, hx, hz) {
            return Ok(false);
        }
        // The tail: `return hx` (skip = beqlr) or `return hx + K2` (skip =
        // beq END). Both captured with hx in r3.
        enum Tail {
            Home,
            AddImmediate(i16),
        }
        let tail = match &function.return_expression {
            Some(Expression::Variable(v)) if v == hx => Tail::Home,
            Some(Expression::Binary { operator: BinaryOperator::Add, left, right })
                if matches!(left.as_ref(), Expression::Variable(v) if v == hx) =>
            {
                let Expression::IntegerLiteral(k2) = right.as_ref() else {
                    return Ok(false);
                };
                let Ok(k2) = i16::try_from(*k2) else {
                    return Ok(false);
                };
                Tail::AddImmediate(k2)
            }
            _ => return Ok(false),
        };
        let Some(hx_register) = self.lookup_general(hx) else {
            return Ok(false);
        };
        if hx_register != 3 {
            return Ok(false);
        }
        let Some(count_register) = self.lookup_general(count) else {
            return Ok(false);
        };
        let head_register = match &head {
            Head::Immediate(_) => None,
            Head::Register(hy) => match self.lookup_general(hy) {
                Some(register) => Some(register),
                None => return Ok(false),
            },
        };
        let pair_low_register = match &then_arm {
            ThenArm::Double => None,
            ThenArm::PairStep(lx) => match self.lookup_general(lx) {
                Some(register) => Some(register),
                None => return Ok(false),
            },
        };
        // -- emit --
        self.output.instructions.push(Instruction::MoveToCountRegister { s: count_register });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: count_register, immediate: 0 });
        let end_label = self.fresh_label();
        match tail {
            Tail::Home => self
                .output
                .instructions
                .push(Instruction::BranchConditionalToLinkRegister { options: 12, condition_bit: 2 }),
            Tail::AddImmediate(_) => self.emit_branch_conditional_to(12, 2, end_label),
        }
        let body_label = self.fresh_label();
        self.bind_label(body_label);
        match &head {
            Head::Immediate(negated_k) => self.output.instructions.push(Instruction::AddImmediateCarryingRecord {
                d: 0,
                a: hx_register,
                immediate: *negated_k,
            }),
            Head::Register(_) => self.output.instructions.push(Instruction::SubtractFromRecord {
                d: 0,
                a: head_register.unwrap(),
                b: hx_register,
            }),
        }
        let else_label = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, else_label); // bge
        match &then_arm {
            ThenArm::Double => {
                self.output.instructions.push(Instruction::Add { d: hx_register, a: hx_register, b: hx_register });
            }
            ThenArm::PairStep(_) => {
                let low = pair_low_register.unwrap();
                self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: low, shift: 31 });
                self.output.instructions.push(Instruction::Add { d: low, a: low, b: low });
                self.output.instructions.push(Instruction::Add { d: 0, a: hx_register, b: 0 });
                self.output.instructions.push(Instruction::Add { d: hx_register, a: hx_register, b: 0 });
            }
        }
        let join_label = self.fresh_label();
        self.emit_branch_to(join_label);
        self.bind_label(else_label);
        self.output.instructions.push(Instruction::Add { d: hx_register, a: 0, b: 0 });
        self.bind_label(join_label);
        self.emit_branch_conditional_to(16, 0, body_label); // bdnz
        if let Tail::AddImmediate(k2) = tail {
            self.bind_label(end_label);
            self.output.instructions.push(Instruction::AddImmediate { d: 3, a: hx_register, immediate: k2 });
        }
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // @N: measured via objprobe after implementation.
        self.output.anonymous_label_bump += 0;
        Ok(true)
    }

    /// The CTR PAIR LOOP (fire 421): e_fmod's core `while(n--)` captured
    /// whole — the 2-word compare-subtract-and-shift walk:
    ///   hz = hx - hy; lz = lx - ly; if (lx < ly) hz -= 1;
    ///   if (hz < 0) { hx = hx+hx+(lx>>31); lx = lx+lx; }
    ///   else        { hx = hz+hz+(lz>>31); lx = lz+lz; }
    /// Emission facts (all measured): the borrow `cmplw lx,ly` hoists
    /// ABOVE both subtracts (they fill its latency); hz/lz take the
    /// FREED COUNT HOME and the next register up, via plain `subf` (no
    /// record — the `hz -= 1` borrow decrement sits between def and
    /// test, so the diamond re-tests with an explicit cmpwi); the then
    /// arm is the fire-420 pair step verbatim; the else arm's
    /// intermediates land DIRECTLY in r3 (hx is not a source there) and
    /// `lx = lz + lz` writes lx's home from lz. @N +0.
    fn try_ctr_pair_loop(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::{LoopKind, Statement};
        if function.return_type != Type::Int
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        // A SCAFFOLD PREFIX may precede the loop (fire 425): the seam is
        // pure concatenation — scaffold ops emit in source order before
        // the mtctr, a loop-crossing sign local takes the next-free
        // register BEFORE the count home frees, and the loop's internal
        // temps allocate around it (hz keeps the freed count home, lz
        // shifts past the sign). Probed forms only: `param &= LOWMASK`
        // (in-place clrlwi), and the sign-extract pair `sign = param &
        // 0x80000000; param ^= sign` (clrrwi + xor).
        let [scaffold @ .., Statement::Loop { kind: LoopKind::While, initializer: None, condition: Some(condition), step: None, body }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        let Expression::PostStep { target, operator: BinaryOperator::Subtract } = condition else {
            return Ok(false);
        };
        let Expression::Variable(count) = target.as_ref() else {
            return Ok(false);
        };
        // The exact captured signature: (int hx, unsigned lx, int hy,
        // unsigned ly, int n) with n LAST — the freed-count-home rule for
        // hz/lz is only measured in that layout.
        let [p_hx, p_lx, p_hy, p_ly, p_n] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if p_hx.parameter_type != Type::Int
            || p_lx.parameter_type != Type::UnsignedInt
            || p_hy.parameter_type != Type::Int
            || p_ly.parameter_type != Type::UnsignedInt
            || p_n.parameter_type != Type::Int
            || p_n.name != *count
        {
            return Ok(false);
        }
        let (hx, lx, hy, ly) = (p_hx.name.as_str(), p_lx.name.as_str(), p_hy.name.as_str(), p_ly.name.as_str());
        // Parse the scaffold prefix (probed forms only).
        enum ScaffoldOp {
            MaskParam { name: String, clear: u8 },
            SignExtract { source: String },
            XorParam { name: String },
        }
        let is_int_parameter = |name: &str| {
            function
                .parameters
                .iter()
                .any(|parameter| parameter.name == name && matches!(parameter.parameter_type, Type::Int | Type::UnsignedInt))
        };
        let mut scaffold_ops: Vec<ScaffoldOp> = Vec::new();
        let mut sign_local: Option<&str> = None;
        for statement in scaffold {
            let Statement::Assign { name, value } = statement else {
                return Ok(false);
            };
            let Expression::Binary { operator, left, right } = value else {
                return Ok(false);
            };
            match operator {
                // param &= (1<<n)-1  →  clrlwi param, param, 32-n (in place).
                BinaryOperator::BitAnd
                    if is_int_parameter(name)
                        && matches!(left.as_ref(), Expression::Variable(v) if v == name) =>
                {
                    let Expression::IntegerLiteral(mask) = right.as_ref() else {
                        return Ok(false);
                    };
                    let mask = *mask as u32;
                    if mask == 0 || !(mask as u64 + 1).is_power_of_two() {
                        return Ok(false);
                    }
                    let clear = mask.leading_zeros() as u8;
                    scaffold_ops.push(ScaffoldOp::MaskParam { name: name.clone(), clear });
                }
                // sign = param & 0x80000000  →  clrrwi sign, param, 31.
                BinaryOperator::BitAnd if !is_int_parameter(name) && sign_local.is_none() => {
                    let Expression::Variable(source) = left.as_ref() else {
                        return Ok(false);
                    };
                    if !is_int_parameter(source)
                        || !matches!(right.as_ref(), Expression::IntegerLiteral(m) if *m as u32 == 0x8000_0000)
                        || !function
                            .locals
                            .iter()
                            .any(|local| local.name == *name && local.declared_type == Type::Int && local.initializer.is_none())
                    {
                        return Ok(false);
                    }
                    sign_local = Some(name.as_str());
                    scaffold_ops.push(ScaffoldOp::SignExtract { source: source.clone() });
                }
                // param ^= sign  →  xor param, param, sign (in place).
                BinaryOperator::BitXor
                    if is_int_parameter(name)
                        && matches!(left.as_ref(), Expression::Variable(v) if v == name) =>
                {
                    if !matches!(right.as_ref(), Expression::Variable(v) if Some(v.as_str()) == sign_local) {
                        return Ok(false);
                    }
                    scaffold_ops.push(ScaffoldOp::XorParam { name: name.clone() });
                }
                _ => return Ok(false),
            }
        }
        // Body: [hz = hx - hy][lz = lx - ly][if (lx < ly) hz -= 1][diamond].
        let [Statement::Assign { name: hz, value: hz_value }, Statement::Assign { name: lz, value: lz_value }, Statement::If { condition: borrow_test, then_body: borrow_then, else_body: borrow_else }, Statement::If { condition: test, then_body, else_body }] =
            body.as_slice()
        else {
            return Ok(false);
        };
        let subtracts = |value: &Expression, from: &str, taken: &str| -> bool {
            let Expression::Binary { operator: BinaryOperator::Subtract, left, right } = value else {
                return false;
            };
            matches!(left.as_ref(), Expression::Variable(v) if v == from)
                && matches!(right.as_ref(), Expression::Variable(v) if v == taken)
        };
        if !subtracts(hz_value, hx, hy) || !subtracts(lz_value, lx, ly) {
            return Ok(false);
        }
        let names_distinct = {
            let mut names = [hx, lx, hy, ly, count.as_str(), hz.as_str(), lz.as_str()];
            names.sort_unstable();
            names.windows(2).all(|pair| pair[0] != pair[1])
        };
        if !names_distinct {
            return Ok(false);
        }
        if let Some(sign) = sign_local {
            if [hx, lx, hy, ly, count.as_str(), hz.as_str(), lz.as_str()].contains(&sign) {
                return Ok(false);
            }
        }
        let is_free_local = |name: &str, declared: Type| {
            function
                .locals
                .iter()
                .any(|local| local.name == name && local.declared_type == declared && local.initializer.is_none())
        };
        if !is_free_local(hz, Type::Int) || !is_free_local(lz, Type::UnsignedInt) {
            return Ok(false);
        }
        // The borrow: if (lx < ly) hz -= 1; (unsigned compare, no else).
        let Expression::Binary { operator: BinaryOperator::Less, left: borrow_left, right: borrow_right } =
            borrow_test
        else {
            return Ok(false);
        };
        if !matches!(borrow_left.as_ref(), Expression::Variable(v) if v == lx)
            || !matches!(borrow_right.as_ref(), Expression::Variable(v) if v == ly)
            || !borrow_else.is_empty()
        {
            return Ok(false);
        }
        let [Statement::Assign { name: decremented, value: decrement }] = borrow_then.as_slice() else {
            return Ok(false);
        };
        if decremented != hz {
            return Ok(false);
        }
        let Expression::Binary { operator: BinaryOperator::Subtract, left: dec_left, right: dec_right } =
            decrement
        else {
            return Ok(false);
        };
        if !matches!(dec_left.as_ref(), Expression::Variable(v) if v == hz)
            || !matches!(dec_right.as_ref(), Expression::IntegerLiteral(1))
        {
            return Ok(false);
        }
        // The diamond: if (hz < 0) {pair step from lx} else {pair step from hz/lz into hx/lx}.
        let Expression::Binary { operator: BinaryOperator::Less, left: test_left, right: test_right } = test
        else {
            return Ok(false);
        };
        if !matches!(test_left.as_ref(), Expression::Variable(v) if v == hz)
            || !matches!(test_right.as_ref(), Expression::IntegerLiteral(0))
        {
            return Ok(false);
        }
        // An arm: high_target = high+high+(low>>31); lx = low+low;
        let pair_step = |statements: &[Statement], high: &str, low: &str| -> bool {
            let [Statement::Assign { name: high_name, value: high_value }, Statement::Assign { name: low_name, value: low_value }] =
                statements
            else {
                return false;
            };
            if high_name != hx || low_name != lx {
                return false;
            }
            let Expression::Binary { operator: BinaryOperator::Add, left: sum, right: carry } = high_value
            else {
                return false;
            };
            let Expression::Binary { operator: BinaryOperator::Add, left: first, right: second } = sum.as_ref()
            else {
                return false;
            };
            if !matches!(first.as_ref(), Expression::Variable(v) if v == high)
                || !matches!(second.as_ref(), Expression::Variable(v) if v == high)
            {
                return false;
            }
            let Expression::Binary { operator: BinaryOperator::ShiftRight, left: shifted, right: amount } =
                carry.as_ref()
            else {
                return false;
            };
            if !matches!(shifted.as_ref(), Expression::Variable(v) if v == low)
                || !matches!(amount.as_ref(), Expression::IntegerLiteral(31))
            {
                return false;
            }
            let Expression::Binary { operator: BinaryOperator::Add, left: low_first, right: low_second } =
                low_value
            else {
                return false;
            };
            matches!(low_first.as_ref(), Expression::Variable(v) if v == low)
                && matches!(low_second.as_ref(), Expression::Variable(v) if v == low)
        };
        if !pair_step(then_body, hx, lx) {
            return Ok(false);
        }
        // The else arm may LEAD with the zero exit `if ((hz | lz) == 0)
        // return K;` — emitted INLINE as `or. r0,hz,lz; bne CONT; li r3,K;
        // blr` (a bare mid-loop return, no exit label; fire 422).
        enum ExitValue {
            Immediate(i16),
            Sign,
        }
        let (early_return, else_step) = match else_body.as_slice() {
            [Statement::If { condition: exit_test, then_body: exit_then, else_body: exit_else }, rest @ ..] => {
                let Expression::Binary { operator: BinaryOperator::Equal, left: or_side, right: zero_side } =
                    exit_test
                else {
                    return Ok(false);
                };
                let Expression::Binary { operator: BinaryOperator::BitOr, left: or_left, right: or_right } =
                    or_side.as_ref()
                else {
                    return Ok(false);
                };
                if !matches!(or_left.as_ref(), Expression::Variable(v) if v == hz)
                    || !matches!(or_right.as_ref(), Expression::Variable(v) if v == lz)
                    || !matches!(zero_side.as_ref(), Expression::IntegerLiteral(0))
                    || !exit_else.is_empty()
                {
                    return Ok(false);
                }
                let exit_value = match exit_then.as_slice() {
                    [Statement::Return(Some(Expression::IntegerLiteral(returned)))] => {
                        let Ok(returned) = i16::try_from(*returned) else {
                            return Ok(false);
                        };
                        ExitValue::Immediate(returned)
                    }
                    [Statement::Return(Some(Expression::Variable(v)))] if Some(v.as_str()) == sign_local => {
                        ExitValue::Sign
                    }
                    _ => return Ok(false),
                };
                (Some(exit_value), rest)
            }
            _ => (None, else_body.as_slice()),
        };
        if !pair_step(else_step, hz, lz) {
            return Ok(false);
        }
        if !matches!(&function.return_expression, Some(Expression::Variable(v)) if v == hx) {
            return Ok(false);
        }
        let (Some(hx_register), Some(lx_register), Some(hy_register), Some(ly_register), Some(count_register)) = (
            self.lookup_general(hx),
            self.lookup_general(lx),
            self.lookup_general(hy),
            self.lookup_general(ly),
            self.lookup_general(count),
        ) else {
            return Ok(false);
        };
        if hx_register != 3 {
            return Ok(false);
        }
        // The sign local takes the next-free register BEFORE the count home
        // frees; hz keeps the freed count home; lz shifts past the sign.
        let sign_register = if sign_local.is_some() { Some(3 + function.parameters.len() as u8) } else { None };
        let hz_register = count_register;
        let lz_register = 3 + function.parameters.len() as u8 + if sign_local.is_some() { 1 } else { 0 };
        if lz_register > 10 {
            return Ok(false);
        }
        // Resolve every scaffold register before any emission.
        enum ResolvedScaffold {
            Mask { register: u8, clear: u8 },
            Extract { source_register: u8 },
            Xor { register: u8 },
        }
        let mut resolved_scaffold = Vec::new();
        for op in &scaffold_ops {
            match op {
                ScaffoldOp::MaskParam { name, clear } => {
                    let Some(register) = self.lookup_general(name) else {
                        return Ok(false);
                    };
                    resolved_scaffold.push(ResolvedScaffold::Mask { register, clear: *clear });
                }
                ScaffoldOp::SignExtract { source } => {
                    let Some(source_register) = self.lookup_general(source) else {
                        return Ok(false);
                    };
                    resolved_scaffold.push(ResolvedScaffold::Extract { source_register });
                }
                ScaffoldOp::XorParam { name } => {
                    let Some(register) = self.lookup_general(name) else {
                        return Ok(false);
                    };
                    resolved_scaffold.push(ResolvedScaffold::Xor { register });
                }
            }
        }
        // -- emit --
        for op in &resolved_scaffold {
            match op {
                ResolvedScaffold::Mask { register, clear } => self
                    .output
                    .instructions
                    .push(Instruction::AndContiguousMask { a: *register, s: *register, begin: *clear, end: 31 }),
                ResolvedScaffold::Extract { source_register } => self.output.instructions.push(
                    Instruction::AndContiguousMask { a: sign_register.unwrap(), s: *source_register, begin: 0, end: 0 },
                ),
                ResolvedScaffold::Xor { register } => self
                    .output
                    .instructions
                    .push(Instruction::Xor { a: *register, s: *register, b: sign_register.unwrap() }),
            }
        }
        self.output.instructions.push(Instruction::MoveToCountRegister { s: count_register });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: count_register, immediate: 0 });
        self.output
            .instructions
            .push(Instruction::BranchConditionalToLinkRegister { options: 12, condition_bit: 2 });
        let body_label = self.fresh_label();
        self.bind_label(body_label);
        self.output.instructions.push(Instruction::CompareLogicalWord { a: lx_register, b: ly_register });
        self.output.instructions.push(Instruction::SubtractFrom { d: hz_register, a: hy_register, b: hx_register });
        self.output.instructions.push(Instruction::SubtractFrom { d: lz_register, a: ly_register, b: lx_register });
        let no_borrow_label = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, no_borrow_label); // bge
        self.output.instructions.push(Instruction::AddImmediate { d: hz_register, a: hz_register, immediate: -1 });
        self.bind_label(no_borrow_label);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: hz_register, immediate: 0 });
        let else_label = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, else_label); // bge
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: lx_register, shift: 31 });
        self.output.instructions.push(Instruction::Add { d: lx_register, a: lx_register, b: lx_register });
        self.output.instructions.push(Instruction::Add { d: 0, a: hx_register, b: 0 });
        self.output.instructions.push(Instruction::Add { d: hx_register, a: hx_register, b: 0 });
        let join_label = self.fresh_label();
        self.emit_branch_to(join_label);
        self.bind_label(else_label);
        if let Some(exit_value) = &early_return {
            self.output.instructions.push(Instruction::OrRecord { a: 0, s: hz_register, b: lz_register });
            let continue_label = self.fresh_label();
            self.emit_branch_conditional_to(4, 2, continue_label); // bne
            match exit_value {
                ExitValue::Immediate(returned) => {
                    self.output.instructions.push(Instruction::load_immediate(3, *returned));
                }
                ExitValue::Sign => {
                    self.output.instructions.push(Instruction::move_register(3, sign_register.unwrap()));
                }
            }
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            self.bind_label(continue_label);
        }
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: 0, s: lz_register, shift: 31 });
        self.output.instructions.push(Instruction::Add { d: lx_register, a: lz_register, b: lz_register });
        self.output.instructions.push(Instruction::Add { d: hx_register, a: hz_register, b: 0 });
        self.output.instructions.push(Instruction::Add { d: hx_register, a: hz_register, b: hx_register });
        self.bind_label(join_label);
        self.emit_branch_conditional_to(16, 0, body_label); // bdnz
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += 0;
        Ok(true)
    }










    /// The WRITEBACK NORM (fire 432, e_fmod's normalize-output tail):
    ///   hx = (hx - HI_BIT) | ((iy + K) << S);
    ///   __HI(x) = hx | sx;  __LO(x) = lx;  return x;
    /// Measured: `hx - HI_BIT` folds to `addis` (high-half subtract);
    /// the stfd spill DELAYS into the int computation; the two punned
    /// stores REORDER BY READINESS (the LO store's lx was ready before
    /// the or-chain, so it emits first); the reload lfd feeds the
    /// return; frame 16 for the one punned double.
    fn try_writeback_norm(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        if function.return_type != Type::Double
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
            || !function.locals.is_empty()
        {
            return Ok(false);
        }
        let [p_x, p_hx, p_lx, p_iy, p_sx] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if p_x.parameter_type != Type::Double
            || p_hx.parameter_type != Type::Int
            || p_lx.parameter_type != Type::UnsignedInt
            || p_iy.parameter_type != Type::Int
            || p_sx.parameter_type != Type::Int
        {
            return Ok(false);
        }
        let (x, hx, lx, iy, sx) =
            (p_x.name.as_str(), p_hx.name.as_str(), p_lx.name.as_str(), p_iy.name.as_str(), p_sx.name.as_str());
        let [Statement::Assign { name: assign_name, value: assign_value }, Statement::Store { target: high_target, value: high_value }, Statement::Store { target: low_target, value: low_value }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        if assign_name != hx {
            return Ok(false);
        }
        // hx = (hx - HI_BIT) | ((iy + K) << S).
        let Expression::Binary { operator: BinaryOperator::BitOr, left: base, right: shifted } = assign_value
        else {
            return Ok(false);
        };
        let Expression::Binary { operator: BinaryOperator::Subtract, left: sub_left, right: sub_right } =
            base.as_ref()
        else {
            return Ok(false);
        };
        if !matches!(sub_left.as_ref(), Expression::Variable(v) if v == hx) {
            return Ok(false);
        }
        let Expression::IntegerLiteral(hi_bit) = sub_right.as_ref() else {
            return Ok(false);
        };
        if *hi_bit & 0xffff != 0 {
            return Ok(false);
        }
        let Ok(addis_immediate) = i16::try_from(-(*hi_bit >> 16)) else {
            return Ok(false);
        };
        let Expression::Binary { operator: BinaryOperator::ShiftLeft, left: sum, right: shift_amount } =
            shifted.as_ref()
        else {
            return Ok(false);
        };
        let Expression::Binary { operator: BinaryOperator::Add, left: add_left, right: add_right } =
            sum.as_ref()
        else {
            return Ok(false);
        };
        if !matches!(add_left.as_ref(), Expression::Variable(v) if v == iy) {
            return Ok(false);
        }
        let Expression::IntegerLiteral(exponent_bias) = add_right.as_ref() else {
            return Ok(false);
        };
        let Ok(exponent_bias) = i16::try_from(*exponent_bias) else {
            return Ok(false);
        };
        let Expression::IntegerLiteral(shift_amount) = shift_amount.as_ref() else {
            return Ok(false);
        };
        let Ok(shift_amount) = u8::try_from(*shift_amount) else {
            return Ok(false);
        };
        if !(1..=31).contains(&shift_amount) {
            return Ok(false);
        }
        // __HI(x) = hx | sx;  __LO(x) = lx;
        if crate::frame::pun_word_offset_pub(high_target, x) != Some(0)
            || crate::frame::pun_word_offset_pub(low_target, x) != Some(4)
        {
            return Ok(false);
        }
        let Expression::Binary { operator: BinaryOperator::BitOr, left: or_left, right: or_right } = high_value
        else {
            return Ok(false);
        };
        if !matches!(or_left.as_ref(), Expression::Variable(v) if v == hx)
            || !matches!(or_right.as_ref(), Expression::Variable(v) if v == sx)
            || !matches!(low_value, Expression::Variable(v) if v == lx)
            || !matches!(&function.return_expression, Some(Expression::Variable(v)) if v == x)
        {
            return Ok(false);
        }
        let (Some(hx_register), Some(lx_register), Some(iy_register), Some(sx_register)) = (
            self.lookup_general(hx),
            self.lookup_general(lx),
            self.lookup_general(iy),
            self.lookup_general(sx),
        ) else {
            return Ok(false);
        };
        // -- emit --
        self.frame_size = 16;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: iy_register, immediate: exponent_bias });
        self.output.instructions.push(Instruction::AddImmediateShifted { d: hx_register, a: hx_register, immediate: addis_immediate });
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: shift_amount });
        // The spill delays into the int computation.
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::Or { a: 0, s: hx_register, b: 0 });
        self.output.instructions.push(Instruction::Or { a: 0, s: 0, b: sx_register });
        // Stores reorder by readiness: lx first.
        self.output.instructions.push(Instruction::StoreWord { s: lx_register, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // @N: measured via objprobe after implementation.
        self.output.anonymous_label_bump += 0;
        Ok(true)
    }

    /// The ALIGN DIAMOND (fire 431, e_fmod's subnormal shift-to-normal):
    ///   if (ix >= K) hx = HI_BIT | (LOW_MASK & hx);
    ///   else { n = K - ix;  // wait: n = -1022 - ix with K = -1022
    ///          if (n <= 31) { hx = (hx<<n)|(lx>>(32-n)); lx <<= n; }
    ///          else { hx = lx << (n-32); lx = 0; } }
    ///   return hx + (int)lx;
    /// Measured: the new hx CONVERGES IN r0 from all three arms (a join
    /// register); `HI_BIT |` folds to `oris` (low half zero); n takes
    /// ix's home via `subfic r5,r5,K`; `32-n` is `subfic r0`; `n-32` is
    /// `addi r0,-32`; lx's in-place `slw` schedules INTO the srw->or
    /// latency; `lx = 0` is li r4,0 and the join adds r0+r4.
    fn try_align_diamond(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        if function.return_type != Type::Int
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        let [p_hx, p_lx, p_ix] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if p_hx.parameter_type != Type::Int
            || p_lx.parameter_type != Type::UnsignedInt
            || p_ix.parameter_type != Type::Int
        {
            return Ok(false);
        }
        let (hx, lx, ix) = (p_hx.name.as_str(), p_lx.name.as_str(), p_ix.name.as_str());
        let [Statement::If { condition: outer, then_body, else_body }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        // Outer: ix >= K (i16).
        let Expression::Binary { operator: BinaryOperator::GreaterEqual, left: outer_left, right: outer_right } =
            outer
        else {
            return Ok(false);
        };
        if !matches!(outer_left.as_ref(), Expression::Variable(v) if v == ix) {
            return Ok(false);
        }
        let Expression::IntegerLiteral(threshold) = outer_right.as_ref() else {
            return Ok(false);
        };
        let Ok(threshold) = i16::try_from(*threshold) else {
            return Ok(false);
        };
        // Then arm: hx = HI_BIT | (LOW_MASK & hx) — oris + clrlwi form.
        let [Statement::Assign { name: then_name, value: then_value }] = then_body.as_slice() else {
            return Ok(false);
        };
        if then_name != hx {
            return Ok(false);
        }
        let Expression::Binary { operator: BinaryOperator::BitOr, left: hi_bit, right: masked } = then_value
        else {
            return Ok(false);
        };
        let Expression::IntegerLiteral(hi_bit) = hi_bit.as_ref() else {
            return Ok(false);
        };
        if *hi_bit & 0xffff != 0 {
            return Ok(false);
        }
        let Ok(oris_immediate) = u16::try_from(*hi_bit >> 16) else {
            return Ok(false);
        };
        let Expression::Binary { operator: BinaryOperator::BitAnd, left: mask, right: mask_source } =
            masked.as_ref()
        else {
            return Ok(false);
        };
        let Expression::IntegerLiteral(mask) = mask.as_ref() else {
            return Ok(false);
        };
        let mask = *mask as u32;
        if mask == 0
            || !(mask as u64 + 1).is_power_of_two()
            || !matches!(mask_source.as_ref(), Expression::Variable(v) if v == hx)
        {
            return Ok(false);
        }
        let clear = mask.leading_zeros() as u8;
        // Else arm: [n = K - ix][the inner shift diamond].
        let [Statement::Assign { name: n, value: n_value }, Statement::If { condition: inner, then_body: small_arm, else_body: big_arm }] =
            else_body.as_slice()
        else {
            return Ok(false);
        };
        if n == hx || n == lx || n == ix {
            return Ok(false);
        }
        if !function
            .locals
            .iter()
            .any(|local| local.name == *n && local.declared_type == Type::Int && local.initializer.is_none())
        {
            return Ok(false);
        }
        let Expression::Binary { operator: BinaryOperator::Subtract, left: n_left, right: n_right } = n_value
        else {
            return Ok(false);
        };
        if !matches!(n_left.as_ref(), Expression::IntegerLiteral(k) if i16::try_from(*k) == Ok(threshold))
            || !matches!(n_right.as_ref(), Expression::Variable(v) if v == ix)
        {
            return Ok(false);
        }
        // Inner: n <= 31.
        let Expression::Binary { operator: BinaryOperator::LessEqual, left: inner_left, right: inner_right } =
            inner
        else {
            return Ok(false);
        };
        if !matches!(inner_left.as_ref(), Expression::Variable(v) if v == n)
            || !matches!(inner_right.as_ref(), Expression::IntegerLiteral(31))
        {
            return Ok(false);
        }
        // Small arm: hx = (hx<<n)|(lx>>(32-n)); lx <<= n;
        let [Statement::Assign { name: sh_name, value: sh_value }, Statement::Assign { name: sl_name, value: sl_value }] =
            small_arm.as_slice()
        else {
            return Ok(false);
        };
        if sh_name != hx || sl_name != lx {
            return Ok(false);
        }
        let Expression::Binary { operator: BinaryOperator::BitOr, left: shifted_high, right: shifted_low } =
            sh_value
        else {
            return Ok(false);
        };
        let shift_of = |expression: &Expression, operator: BinaryOperator, value: &str| -> Option<()> {
            let Expression::Binary { operator: found, left, right } = expression else {
                return None;
            };
            if *found != operator || !matches!(left.as_ref(), Expression::Variable(v) if v == value) {
                return None;
            }
            match right.as_ref() {
                Expression::Variable(v) if v == n => Some(()),
                _ => None,
            }
        };
        if shift_of(shifted_high.as_ref(), BinaryOperator::ShiftLeft, hx).is_none() {
            return Ok(false);
        }
        {
            let Expression::Binary { operator: BinaryOperator::ShiftRight, left: low_source, right: amount } =
                shifted_low.as_ref()
            else {
                return Ok(false);
            };
            if !matches!(low_source.as_ref(), Expression::Variable(v) if v == lx) {
                return Ok(false);
            }
            let Expression::Binary { operator: BinaryOperator::Subtract, left: from, right: taken } =
                amount.as_ref()
            else {
                return Ok(false);
            };
            if !matches!(from.as_ref(), Expression::IntegerLiteral(32))
                || !matches!(taken.as_ref(), Expression::Variable(v) if v == n)
            {
                return Ok(false);
            }
        }
        if shift_of(sl_value, BinaryOperator::ShiftLeft, lx).is_none() {
            return Ok(false);
        }
        // Big arm: hx = lx << (n-32); lx = 0;
        let [Statement::Assign { name: bh_name, value: bh_value }, Statement::Assign { name: bl_name, value: bl_value }] =
            big_arm.as_slice()
        else {
            return Ok(false);
        };
        if bh_name != hx || bl_name != lx || !matches!(bl_value, Expression::IntegerLiteral(0)) {
            return Ok(false);
        }
        {
            let Expression::Binary { operator: BinaryOperator::ShiftLeft, left: low_source, right: amount } =
                bh_value
            else {
                return Ok(false);
            };
            if !matches!(low_source.as_ref(), Expression::Variable(v) if v == lx) {
                return Ok(false);
            }
            let Expression::Binary { operator: BinaryOperator::Subtract, left: from, right: taken } =
                amount.as_ref()
            else {
                return Ok(false);
            };
            if !matches!(from.as_ref(), Expression::Variable(v) if v == n)
                || !matches!(taken.as_ref(), Expression::IntegerLiteral(32))
            {
                return Ok(false);
            }
        }
        // Return: hx + (int)lx.
        let Some(Expression::Binary { operator: BinaryOperator::Add, left: ret_left, right: ret_right }) =
            &function.return_expression
        else {
            return Ok(false);
        };
        if !matches!(ret_left.as_ref(), Expression::Variable(v) if v == hx) {
            return Ok(false);
        }
        let Expression::Cast { target_type: Type::Int, operand } = ret_right.as_ref() else {
            return Ok(false);
        };
        if !matches!(operand.as_ref(), Expression::Variable(v) if v == lx) {
            return Ok(false);
        }
        let (Some(hx_register), Some(lx_register), Some(ix_register)) =
            (self.lookup_general(hx), self.lookup_general(lx), self.lookup_general(ix))
        else {
            return Ok(false);
        };
        // -- emit --
        self.output.instructions.push(Instruction::CompareWordImmediate { a: ix_register, immediate: threshold });
        let else_label = self.fresh_label();
        self.emit_branch_conditional_to(12, 0, else_label); // blt
        self.output.instructions.push(Instruction::ClearLeftImmediate { a: 0, s: hx_register, clear });
        self.output.instructions.push(Instruction::OrImmediateShifted { a: 0, s: 0, immediate: oris_immediate });
        let join_label = self.fresh_label();
        self.emit_branch_to(join_label);
        // n takes ix's home: subfic r5, r5, K.
        self.bind_label(else_label);
        self.output.instructions.push(Instruction::SubtractFromImmediate { d: ix_register, a: ix_register, immediate: threshold });
        self.output.instructions.push(Instruction::CompareWordImmediate { a: ix_register, immediate: 31 });
        let big_label = self.fresh_label();
        self.emit_branch_conditional_to(12, 1, big_label); // bgt
        self.output.instructions.push(Instruction::SubtractFromImmediate { d: 0, a: ix_register, immediate: 32 });
        self.output.instructions.push(Instruction::ShiftLeftWord { a: hx_register, s: hx_register, b: ix_register });
        self.output.instructions.push(Instruction::ShiftRightWord { a: 0, s: lx_register, b: 0 });
        self.output.instructions.push(Instruction::ShiftLeftWord { a: lx_register, s: lx_register, b: ix_register });
        self.output.instructions.push(Instruction::Or { a: 0, s: hx_register, b: 0 });
        self.emit_branch_to(join_label);
        self.bind_label(big_label);
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: ix_register, immediate: -32 });
        self.output.instructions.push(Instruction::ShiftLeftWord { a: 0, s: lx_register, b: 0 });
        self.output.instructions.push(Instruction::load_immediate(lx_register, 0));
        self.bind_label(join_label);
        self.output.instructions.push(Instruction::Add { d: 3, a: 0, b: lx_register });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += 0;
        Ok(true)
    }

    /// The PUNNED PAIR LADDER (fire 429/430, e_fmod's |x|<=|y| purge fed
    /// from DOUBLE params): the frame/int marriage. `int f(double x,
    /// double y)` punning hx/lx/hy/ly then the fire-427 ladder.
    /// Measured FRAMED rules (contrast the frameless captures): arms
    /// JOIN at the shared epilogue (`li; b JOIN` — inline blr is a
    /// frameless-only behavior); punned loads emit in first-use order
    /// with ly DELAYED past the cmpw into its branch latency, reusing
    /// dead hx's r0; frame 32 = 8 linkage + 2x8 doubles, spilled at
    /// 8/16(r1); no stmw.
    fn try_punned_pair_ladder(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        if function.return_type != Type::Int
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        let [p_x, p_y] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if p_x.parameter_type != Type::Double || p_y.parameter_type != Type::Double {
            return Ok(false);
        }
        let (x, y) = (p_x.name.as_str(), p_y.name.as_str());
        if x == y {
            return Ok(false);
        }
        // The four extracts in e_fmod's order: x-high, x-low, y-high, y-low.
        let [Statement::Assign { name: hx, value: hx_value }, Statement::Assign { name: lx, value: lx_value }, Statement::Assign { name: hy, value: hy_value }, Statement::Assign { name: ly, value: ly_value }, Statement::If { condition: outer, then_body, else_body }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        if crate::frame::pun_word_offset_pub(hx_value, x) != Some(0)
            || crate::frame::pun_word_offset_pub(lx_value, x) != Some(4)
            || crate::frame::pun_word_offset_pub(hy_value, y) != Some(0)
            || crate::frame::pun_word_offset_pub(ly_value, y) != Some(4)
            || !else_body.is_empty()
        {
            return Ok(false);
        }
        let names_distinct = {
            let mut names = [x, y, hx.as_str(), lx.as_str(), hy.as_str(), ly.as_str()];
            names.sort_unstable();
            names.windows(2).all(|pair| pair[0] != pair[1])
        };
        if !names_distinct {
            return Ok(false);
        }
        let typed_local = |name: &str, declared: Type| {
            function
                .locals
                .iter()
                .any(|local| local.name == name && local.declared_type == declared && local.initializer.is_none())
        };
        if !typed_local(hx, Type::Int)
            || !typed_local(hy, Type::Int)
            || !typed_local(lx, Type::UnsignedInt)
            || !typed_local(ly, Type::UnsignedInt)
        {
            return Ok(false);
        }
        // The ladder (fire 427's shape over the punned locals).
        let is_pair = |expression: &Expression, operator: BinaryOperator, a: &str, b: &str| -> bool {
            let Expression::Binary { operator: found, left, right } = expression else {
                return false;
            };
            *found == operator
                && matches!(left.as_ref(), Expression::Variable(v) if v == a)
                && matches!(right.as_ref(), Expression::Variable(v) if v == b)
        };
        if !is_pair(outer, BinaryOperator::LessEqual, hx, hy) {
            return Ok(false);
        }
        let [Statement::If { condition: or_test, then_body: or_then, else_body: or_else }, Statement::If { condition: eq_test, then_body: eq_then, else_body: eq_else }] =
            then_body.as_slice()
        else {
            return Ok(false);
        };
        if !or_else.is_empty() || !eq_else.is_empty() {
            return Ok(false);
        }
        let Expression::Binary { operator: BinaryOperator::LogicalOr, left: or_left, right: or_right } =
            or_test
        else {
            return Ok(false);
        };
        if !is_pair(or_left.as_ref(), BinaryOperator::Less, hx, hy)
            || !is_pair(or_right.as_ref(), BinaryOperator::Less, lx, ly)
            || !is_pair(eq_test, BinaryOperator::Equal, lx, ly)
        {
            return Ok(false);
        }
        let arm_return = |statements: &[Statement]| -> Option<i16> {
            let [Statement::Return(Some(Expression::IntegerLiteral(value)))] = statements else {
                return None;
            };
            i16::try_from(*value).ok()
        };
        let (Some(k1), Some(k2)) = (arm_return(or_then), arm_return(eq_then)) else {
            return Ok(false);
        };
        let Some(Expression::IntegerLiteral(k3)) = &function.return_expression else {
            return Ok(false);
        };
        let Ok(k3) = i16::try_from(*k3) else {
            return Ok(false);
        };
        // -- emit (registers per the capture: hx r0, hy r3, lx r4, ly r0) --
        self.frame_size = 32;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 2, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 16 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::CompareWord { a: 0, b: 3 });
        // ly's load DELAYS into the compare->branch latency, reusing dead hx's r0.
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        let end_label = self.fresh_label();
        self.emit_branch_conditional_to(12, 1, end_label); // bgt
        let first_return_label = self.fresh_label();
        self.emit_branch_conditional_to(12, 0, first_return_label); // blt
        self.output.instructions.push(Instruction::CompareLogicalWord { a: 4, b: 0 });
        let equality_label = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, equality_label); // bge
        let join_label = self.fresh_label();
        self.bind_label(first_return_label);
        self.output.instructions.push(Instruction::load_immediate(3, k1));
        self.emit_branch_to(join_label);
        self.bind_label(equality_label);
        self.emit_branch_conditional_to(4, 2, end_label); // bne (CR0 reused from the cmplw)
        self.output.instructions.push(Instruction::load_immediate(3, k2));
        self.emit_branch_to(join_label);
        self.bind_label(end_label);
        self.output.instructions.push(Instruction::load_immediate(3, k3));
        self.bind_label(join_label);
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 32 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // @N: measured via objprobe — the real extab lands at @12 (mwcc
        // numbers the ladder's internal labels).
        self.output.anonymous_label_bump += 7;
        Ok(true)
    }

    /// The SIGN-INDEXED DOUBLE RETURN (fire 428, e_fmod's Zero[] exit):
    /// `return Zero[(unsigned)sx >> 31];` for a `static double Zero[]`.
    /// Measured: the index `(sx>>31)<<3` FUSES into one rotate-mask
    /// (`rlwinm r0,sx,4,28,28`); the base is a lis/addi ADDR16_HA/LO
    /// pair on the (local) array symbol — .data, NOT sdata, despite the
    /// 16-byte size; the load is `lfdx f1,lo,index`. Register slots per
    /// the capture: ha -> r4, lo -> r3 (sx's home, dead after the
    /// rlwinm), index -> r0.
    fn try_indexed_double_return(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::Double
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
            || !function.locals.is_empty()
            || !function.statements.is_empty()
        {
            return Ok(false);
        }
        let [p_sx] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if p_sx.parameter_type != Type::Int {
            return Ok(false);
        }
        let sx = p_sx.name.as_str();
        let Some(Expression::Index { base, index }) = &function.return_expression else {
            return Ok(false);
        };
        let Expression::Variable(array) = base.as_ref() else {
            return Ok(false);
        };
        if array == sx {
            return Ok(false);
        }
        let Expression::Binary { operator: BinaryOperator::ShiftRight, left: shifted, right: amount } =
            index.as_ref()
        else {
            return Ok(false);
        };
        let Expression::Cast { target_type: Type::UnsignedInt, operand } = shifted.as_ref() else {
            return Ok(false);
        };
        if !matches!(operand.as_ref(), Expression::Variable(v) if v == sx)
            || !matches!(amount.as_ref(), Expression::IntegerLiteral(31))
        {
            return Ok(false);
        }
        let Some(sx_register) = self.lookup_general(sx) else {
            return Ok(false);
        };
        if sx_register != 3 {
            return Ok(false);
        }
        let array = array.clone();
        // -- emit --
        self.emit_address_high(4, &array);
        // (sx >> 31) << 3 in one rotate-mask: rotate left 4, keep bit 28.
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: sx_register, shift: 4, begin: 28, end: 28 });
        self.record_relocation(RelocationKind::Addr16Lo, &array);
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 4, immediate: 0 });
        self.output.instructions.push(Instruction::LoadFloatDoubleIndexed { d: 1, a: 3, b: 0 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += 0;
        Ok(true)
    }

    /// The EARLY LADDER (fire 427, e_fmod's |x|<=|y| purge):
    ///   if (hx <= hy) { if ((hx < hy) || (lx < ly)) return K1;
    ///                   if (lx == ly) return K2; }  return K3;
    /// Measured: ONE `cmplw lx,ly` serves BOTH the `||` arm and the
    /// later `==` test — CR0 survives the branch between them (compare
    /// CSE across branches); the `||` short-circuits through `blt` into
    /// the shared return; every return is inline (li; blr — no join).
    fn try_early_ladder(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        if function.return_type != Type::Int
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
            || !function.locals.is_empty()
        {
            return Ok(false);
        }
        let [p_hx, p_lx, p_hy, p_ly] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if p_hx.parameter_type != Type::Int
            || p_lx.parameter_type != Type::UnsignedInt
            || p_hy.parameter_type != Type::Int
            || p_ly.parameter_type != Type::UnsignedInt
        {
            return Ok(false);
        }
        let (hx, lx, hy, ly) = (p_hx.name.as_str(), p_lx.name.as_str(), p_hy.name.as_str(), p_ly.name.as_str());
        let [Statement::If { condition: outer, then_body, else_body }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if !else_body.is_empty() {
            return Ok(false);
        }
        let is_pair = |expression: &Expression, operator: BinaryOperator, a: &str, b: &str| -> bool {
            let Expression::Binary { operator: found, left, right } = expression else {
                return false;
            };
            *found == operator
                && matches!(left.as_ref(), Expression::Variable(v) if v == a)
                && matches!(right.as_ref(), Expression::Variable(v) if v == b)
        };
        if !is_pair(outer, BinaryOperator::LessEqual, hx, hy) {
            return Ok(false);
        }
        let [Statement::If { condition: or_test, then_body: or_then, else_body: or_else }, Statement::If { condition: eq_test, then_body: eq_then, else_body: eq_else }] =
            then_body.as_slice()
        else {
            return Ok(false);
        };
        if !or_else.is_empty() || !eq_else.is_empty() {
            return Ok(false);
        }
        let Expression::Binary { operator: BinaryOperator::LogicalOr, left: or_left, right: or_right } =
            or_test
        else {
            return Ok(false);
        };
        if !is_pair(or_left.as_ref(), BinaryOperator::Less, hx, hy)
            || !is_pair(or_right.as_ref(), BinaryOperator::Less, lx, ly)
            || !is_pair(eq_test, BinaryOperator::Equal, lx, ly)
        {
            return Ok(false);
        }
        let arm_return = |statements: &[Statement]| -> Option<i16> {
            let [Statement::Return(Some(Expression::IntegerLiteral(value)))] = statements else {
                return None;
            };
            i16::try_from(*value).ok()
        };
        let (Some(k1), Some(k2)) = (arm_return(or_then), arm_return(eq_then)) else {
            return Ok(false);
        };
        let Some(Expression::IntegerLiteral(k3)) = &function.return_expression else {
            return Ok(false);
        };
        let Ok(k3) = i16::try_from(*k3) else {
            return Ok(false);
        };
        let (Some(hx_register), Some(lx_register), Some(hy_register), Some(ly_register)) = (
            self.lookup_general(hx),
            self.lookup_general(lx),
            self.lookup_general(hy),
            self.lookup_general(ly),
        ) else {
            return Ok(false);
        };
        // -- emit --
        self.output.instructions.push(Instruction::CompareWord { a: hx_register, b: hy_register });
        let end_label = self.fresh_label();
        self.emit_branch_conditional_to(12, 1, end_label); // bgt
        let first_return_label = self.fresh_label();
        self.emit_branch_conditional_to(12, 0, first_return_label); // blt (the || short-circuit)
        self.output.instructions.push(Instruction::CompareLogicalWord { a: lx_register, b: ly_register });
        let equality_label = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, equality_label); // bge
        self.bind_label(first_return_label);
        self.output.instructions.push(Instruction::load_immediate(3, k1));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // The equality test REUSES the cmplw's CR0 (no second compare).
        self.bind_label(equality_label);
        self.emit_branch_conditional_to(4, 2, end_label); // bne
        self.output.instructions.push(Instruction::load_immediate(3, k2));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(end_label);
        self.output.instructions.push(Instruction::load_immediate(3, k3));
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // @N: measured via objprobe after implementation.
        self.output.anonymous_label_bump += 0;
        Ok(true)
    }

    /// The ILOGB DIAMOND (fire 426, e_fmod's exponent extract): rotated
    /// loops NEST INTO IF-ARMS by concatenation with per-arm register
    /// context —
    ///   if (hx < BIG) { if (hx == 0) FOR-LOOP(lx) else FOR-LOOP(hx<<A) }
    ///   else ix = (hx >> 20) - K;  return ix;
    /// Measured: ix lands DIRECTLY in r3 in every arm (hx is dead inside
    /// them, killing the standalone loop's trailing mr); each arm ends
    /// with its own inline blr (no join); r0 double-duties (the lis
    /// bound dies at the cmpw, arm 2 reuses r0 for its shift temp); the
    /// arm-2 shift init emits BEFORE the li that overwrites hx's home.
    fn try_ilogb_diamond(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::{LoopKind, Statement};
        if function.return_type != Type::Int
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        let [p_hx, p_lx] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if p_hx.parameter_type != Type::Int || p_lx.parameter_type != Type::UnsignedInt {
            return Ok(false);
        }
        let (hx, lx) = (p_hx.name.as_str(), p_lx.name.as_str());
        let [Statement::If { condition: outer_test, then_body, else_body }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        // Outer: hx < BIG (lis-only constant).
        let Expression::Binary { operator: BinaryOperator::Less, left: outer_left, right: outer_right } =
            outer_test
        else {
            return Ok(false);
        };
        if !matches!(outer_left.as_ref(), Expression::Variable(v) if v == hx) {
            return Ok(false);
        }
        let Expression::IntegerLiteral(bound) = outer_right.as_ref() else {
            return Ok(false);
        };
        if *bound & 0xffff != 0 {
            return Ok(false);
        }
        let Ok(bound_high) = i16::try_from(*bound >> 16) else {
            return Ok(false);
        };
        // Inner diamond: if (hx == 0) loop-over-lx else loop-over-(hx<<A).
        let [Statement::If { condition: inner_test, then_body: zero_arm, else_body: shift_arm }] =
            then_body.as_slice()
        else {
            return Ok(false);
        };
        let Expression::Binary { operator: BinaryOperator::Equal, left: inner_left, right: inner_right } =
            inner_test
        else {
            return Ok(false);
        };
        if !matches!(inner_left.as_ref(), Expression::Variable(v) if v == hx)
            || !matches!(inner_right.as_ref(), Expression::IntegerLiteral(0))
        {
            return Ok(false);
        }
        // An arm loop: for (ix = K, i = SRC; i > 0; i <<= 1) ix -= 1;
        enum ArmSource {
            InPlaceLow,
            ShiftOfHigh(u8),
        }
        struct ArmLoop {
            start: i16,
            source: ArmSource,
        }
        let mut result_local: Option<String> = None;
        let mut counter_local: Option<String> = None;
        let mut parse_arm = |statements: &[Statement]| -> Option<ArmLoop> {
            let [Statement::Loop { kind: LoopKind::For, initializer: Some(init), condition: Some(cond), step: Some(step), body }] =
                statements
            else {
                return None;
            };
            // The comma init: (ix = K, i = SRC).
            let Expression::Comma { left: first, right: second } = init else {
                return None;
            };
            let Expression::Assign { target: ix_target, value: ix_value } = first.as_ref() else {
                return None;
            };
            let Expression::Variable(ix_name) = ix_target.as_ref() else {
                return None;
            };
            let Expression::IntegerLiteral(start) = ix_value.as_ref() else {
                return None;
            };
            let start = i16::try_from(*start).ok()?;
            let Expression::Assign { target: i_target, value: i_value } = second.as_ref() else {
                return None;
            };
            let Expression::Variable(i_name) = i_target.as_ref() else {
                return None;
            };
            let source = match i_value.as_ref() {
                Expression::Variable(v) if v == lx => ArmSource::InPlaceLow,
                Expression::Binary { operator: BinaryOperator::ShiftLeft, left, right } => {
                    if !matches!(left.as_ref(), Expression::Variable(v) if v == hx) {
                        return None;
                    }
                    let Expression::IntegerLiteral(amount) = right.as_ref() else {
                        return None;
                    };
                    ArmSource::ShiftOfHigh(u8::try_from(*amount).ok().filter(|a| (1..=31).contains(a))?)
                }
                _ => return None,
            };
            // Locals consistent across arms; distinct from the params.
            if ix_name == hx || ix_name == lx || i_name == hx || i_name == lx || ix_name == i_name {
                return None;
            }
            match (&result_local, &counter_local) {
                (None, None) => {
                    result_local = Some(ix_name.clone());
                    counter_local = Some(i_name.clone());
                }
                (Some(result), Some(counter)) => {
                    if result != ix_name || counter != i_name {
                        return None;
                    }
                }
                _ => return None,
            }
            // Condition: i > 0. Step: i <<= 1. Body: ix -= 1.
            let Expression::Binary { operator: BinaryOperator::Greater, left: cond_left, right: cond_right } =
                cond
            else {
                return None;
            };
            if !matches!(cond_left.as_ref(), Expression::Variable(v) if v == i_name)
                || !matches!(cond_right.as_ref(), Expression::IntegerLiteral(0))
            {
                return None;
            }
            let Expression::Assign { target: step_target, value: step_value } = step else {
                return None;
            };
            let Expression::Binary { operator: BinaryOperator::ShiftLeft, left: step_left, right: step_right } =
                step_value.as_ref()
            else {
                return None;
            };
            if !matches!(step_target.as_ref(), Expression::Variable(v) if v == i_name)
                || !matches!(step_left.as_ref(), Expression::Variable(v) if v == i_name)
                || !matches!(step_right.as_ref(), Expression::IntegerLiteral(1))
            {
                return None;
            }
            let [Statement::Assign { name: body_name, value: body_value }] = body.as_slice() else {
                return None;
            };
            let Expression::Binary { operator: BinaryOperator::Subtract, left: body_left, right: body_right } =
                body_value
            else {
                return None;
            };
            if body_name != ix_name
                || !matches!(body_left.as_ref(), Expression::Variable(v) if v == ix_name)
                || !matches!(body_right.as_ref(), Expression::IntegerLiteral(1))
            {
                return None;
            }
            Some(ArmLoop { start, source })
        };
        let Some(zero_loop) = parse_arm(zero_arm) else {
            return Ok(false);
        };
        let Some(shift_loop) = parse_arm(shift_arm) else {
            return Ok(false);
        };
        if !matches!(zero_loop.source, ArmSource::InPlaceLow)
            || !matches!(shift_loop.source, ArmSource::ShiftOfHigh(_))
        {
            return Ok(false);
        }
        // The else arm: ix = (hx >> S) - K.
        let [Statement::Assign { name: else_name, value: else_value }] = else_body.as_slice() else {
            return Ok(false);
        };
        if Some(else_name.as_str()) != result_local.as_deref() {
            return Ok(false);
        }
        let Expression::Binary { operator: BinaryOperator::Subtract, left: shifted, right: offset } =
            else_value
        else {
            return Ok(false);
        };
        let Expression::Binary { operator: BinaryOperator::ShiftRight, left: shift_source, right: shift_amount } =
            shifted.as_ref()
        else {
            return Ok(false);
        };
        if !matches!(shift_source.as_ref(), Expression::Variable(v) if v == hx) {
            return Ok(false);
        }
        let Expression::IntegerLiteral(else_shift) = shift_amount.as_ref() else {
            return Ok(false);
        };
        let Ok(else_shift) = u8::try_from(*else_shift) else {
            return Ok(false);
        };
        if !(1..=31).contains(&else_shift) {
            return Ok(false);
        }
        let Expression::IntegerLiteral(else_offset) = offset.as_ref() else {
            return Ok(false);
        };
        let Ok(negated_offset) = i16::try_from(-*else_offset) else {
            return Ok(false);
        };
        if !matches!(&function.return_expression, Some(Expression::Variable(v)) if Some(v.as_str()) == result_local.as_deref())
        {
            return Ok(false);
        }
        let (Some(hx_register), Some(lx_register)) = (self.lookup_general(hx), self.lookup_general(lx))
        else {
            return Ok(false);
        };
        if hx_register != 3 {
            return Ok(false);
        }
        // -- emit --
        self.output.instructions.push(Instruction::AddImmediateShifted { d: 0, a: 0, immediate: bound_high });
        self.output.instructions.push(Instruction::CompareWord { a: hx_register, b: 0 });
        let else_label = self.fresh_label();
        self.emit_branch_conditional_to(4, 0, else_label); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: hx_register, immediate: 0 });
        let shift_arm_label = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, shift_arm_label); // bne
        // Arm 1: the loop over lx, ix in r3, counter in lx's home.
        self.output.instructions.push(Instruction::load_immediate(3, zero_loop.start));
        let test1 = self.fresh_label();
        self.emit_branch_to(test1);
        let body1 = self.fresh_label();
        self.bind_label(body1);
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: lx_register, s: lx_register, shift: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -1 });
        self.bind_label(test1);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: lx_register, immediate: 0 });
        self.emit_branch_conditional_to(12, 1, body1); // bgt
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // Arm 2: the loop over hx<<A; the shift temp rides r0 (the bound
        // is dead), and its init emits BEFORE the li overwrites r3.
        self.bind_label(shift_arm_label);
        let ArmSource::ShiftOfHigh(amount) = shift_loop.source else {
            return Ok(false);
        };
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: hx_register, shift: amount });
        self.output.instructions.push(Instruction::load_immediate(3, shift_loop.start));
        let test2 = self.fresh_label();
        self.emit_branch_to(test2);
        let body2 = self.fresh_label();
        self.bind_label(body2);
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: 0, s: 0, shift: 1 });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: -1 });
        self.bind_label(test2);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 1, body2); // bgt
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // The else arm: srawi + addi in place.
        self.bind_label(else_label);
        self.output
            .instructions
            .push(Instruction::ShiftRightAlgebraicImmediate { a: 3, s: hx_register, shift: else_shift });
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 3, immediate: negated_offset });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // @N: measured via objprobe after implementation.
        self.output.anonymous_label_bump += 0;
        Ok(true)
    }

    /// The NORMALIZE LOOP (fire 424, e_fmod's tail loop): a NON-counted
    /// `while (hx < BIG) { hx = hx+hx+(lx>>31); lx = lx+lx; iy -= 1; }`
    /// with `return hx + iy` — rotated form with the big bound hoisted
    /// `lis r0, BIG>>16` BEFORE the loop. r0 stays OCCUPIED across the
    /// body, so the carry temp takes the next free register after the
    /// params; the iy decrement schedules INTO the add latency (between
    /// `add rT,hx,rT` and `add hx,hx,rT`). Bound gated to lis-only
    /// constants (low half zero, not a cmpwi immediate).
    fn try_norm_loop(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::{LoopKind, Statement};
        if function.return_type != Type::Int
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
            || !function.locals.is_empty()
        {
            return Ok(false);
        }
        let [p_hx, p_lx, p_iy] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if p_hx.parameter_type != Type::Int
            || p_lx.parameter_type != Type::UnsignedInt
            || p_iy.parameter_type != Type::Int
        {
            return Ok(false);
        }
        let (hx, lx, iy) = (p_hx.name.as_str(), p_lx.name.as_str(), p_iy.name.as_str());
        if hx == lx || hx == iy || lx == iy {
            return Ok(false);
        }
        let [Statement::Loop { kind: LoopKind::While, initializer: None, condition: Some(condition), step: None, body }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        // The condition: hx < BIG, BIG a lis-only constant (low half 0).
        let Expression::Binary { operator: BinaryOperator::Less, left: test_left, right: test_right } =
            condition
        else {
            return Ok(false);
        };
        if !matches!(test_left.as_ref(), Expression::Variable(v) if v == hx) {
            return Ok(false);
        }
        let Expression::IntegerLiteral(bound) = test_right.as_ref() else {
            return Ok(false);
        };
        let bound = *bound;
        if bound & 0xffff != 0 {
            return Ok(false);
        }
        let Ok(bound_high) = i16::try_from(bound >> 16) else {
            return Ok(false);
        };
        // The body: [hx = hx+hx+(lx>>31)][lx = lx+lx][iy = iy-1].
        let [Statement::Assign { name: high_name, value: high_value }, Statement::Assign { name: low_name, value: low_value }, Statement::Assign { name: dec_name, value: dec_value }] =
            body.as_slice()
        else {
            return Ok(false);
        };
        if high_name != hx || low_name != lx || dec_name != iy {
            return Ok(false);
        }
        let Expression::Binary { operator: BinaryOperator::Add, left: sum, right: carry } = high_value
        else {
            return Ok(false);
        };
        let Expression::Binary { operator: BinaryOperator::Add, left: first, right: second } = sum.as_ref()
        else {
            return Ok(false);
        };
        let Expression::Binary { operator: BinaryOperator::ShiftRight, left: shifted, right: amount } =
            carry.as_ref()
        else {
            return Ok(false);
        };
        if !matches!(first.as_ref(), Expression::Variable(v) if v == hx)
            || !matches!(second.as_ref(), Expression::Variable(v) if v == hx)
            || !matches!(shifted.as_ref(), Expression::Variable(v) if v == lx)
            || !matches!(amount.as_ref(), Expression::IntegerLiteral(31))
        {
            return Ok(false);
        }
        let Expression::Binary { operator: BinaryOperator::Add, left: low_first, right: low_second } =
            low_value
        else {
            return Ok(false);
        };
        if !matches!(low_first.as_ref(), Expression::Variable(v) if v == lx)
            || !matches!(low_second.as_ref(), Expression::Variable(v) if v == lx)
        {
            return Ok(false);
        }
        let Expression::Binary { operator: BinaryOperator::Subtract, left: dec_left, right: dec_right } =
            dec_value
        else {
            return Ok(false);
        };
        if !matches!(dec_left.as_ref(), Expression::Variable(v) if v == iy)
            || !matches!(dec_right.as_ref(), Expression::IntegerLiteral(1))
        {
            return Ok(false);
        }
        // The tail: return hx + iy.
        let Some(Expression::Binary { operator: BinaryOperator::Add, left: ret_left, right: ret_right }) =
            &function.return_expression
        else {
            return Ok(false);
        };
        if !matches!(ret_left.as_ref(), Expression::Variable(v) if v == hx)
            || !matches!(ret_right.as_ref(), Expression::Variable(v) if v == iy)
        {
            return Ok(false);
        }
        let (Some(hx_register), Some(lx_register), Some(iy_register)) =
            (self.lookup_general(hx), self.lookup_general(lx), self.lookup_general(iy))
        else {
            return Ok(false);
        };
        if hx_register != 3 {
            return Ok(false);
        }
        // The carry temp: next free past the params (r0 holds the bound).
        let temp = 3 + function.parameters.len() as u8;
        if temp > 10 {
            return Ok(false);
        }
        // -- emit --
        self.output.instructions.push(Instruction::AddImmediateShifted { d: 0, a: 0, immediate: bound_high });
        let test_label = self.fresh_label();
        self.emit_branch_to(test_label);
        let body_label = self.fresh_label();
        self.bind_label(body_label);
        self.output.instructions.push(Instruction::ShiftRightLogicalImmediate { a: temp, s: lx_register, shift: 31 });
        self.output.instructions.push(Instruction::Add { d: lx_register, a: lx_register, b: lx_register });
        self.output.instructions.push(Instruction::Add { d: temp, a: hx_register, b: temp });
        self.output.instructions.push(Instruction::AddImmediate { d: iy_register, a: iy_register, immediate: -1 });
        self.output.instructions.push(Instruction::Add { d: hx_register, a: hx_register, b: temp });
        self.bind_label(test_label);
        self.output.instructions.push(Instruction::CompareWord { a: hx_register, b: 0 });
        self.emit_branch_conditional_to(12, 0, body_label); // blt
        self.output.instructions.push(Instruction::Add { d: 3, a: hx_register, b: iy_register });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.output.anonymous_label_bump += 0;
        Ok(true)
    }

    /// The ROTATED LOOP (fire 413, the e_fmod ilogb family): mwcc emits
    /// non-counted loops as `init; b TEST; BODY: [step][body]; TEST:
    /// cond; b<positive> BODY; [mr]` with NO unrolling (counted loops
    /// take the ctr/unroll machinery instead — deferred). Registers per
    /// the captures: params in place; a condition-only computed value
    /// takes r0 (even across the backward branch); the returned local
    /// takes a param home freed during init, else the next free.
    fn try_rotated_loop(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::{LoopKind, Statement};
        if !matches!(function.return_type, Type::Int | Type::Void)
            || !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function_makes_call(function)
        {
            return Ok(false);
        }
        let [Statement::Loop { kind, initializer, condition: Some(condition), step, body }] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        if function.return_type != Type::Void && !matches!(&function.return_expression, Some(Expression::Variable(_))) {
            return Ok(false);
        }
        // Locals: uninitialized (bound via the comma init), or initialized
        // with a small constant (an init-plan entry).
        let mut local_constant_inits: Vec<(&str, i16)> = Vec::new();
        for local in &function.locals {
            if local.declared_type != Type::Int || local.array_length.is_some() {
                return Ok(false);
            }
            if let Some(init) = &local.initializer {
                let Some(constant) =
                    crate::analysis::constant_value(init).and_then(|k| i16::try_from(k).ok())
                else {
                    return Ok(false);
                };
                local_constant_inits.push((local.name.as_str(), constant));
            }
        }
        // Homes: params in their registers.
        let mut homes: Vec<(String, u8)> = Vec::new();
        let mut char_pointers: Vec<String> = Vec::new();
        for parameter in &function.parameters {
            match parameter.parameter_type {
                Type::Int => {}
                Type::Pointer(Pointee::Char) => char_pointers.push(parameter.name.clone()),
                _ => return Ok(false),
            }
            let Some(register) = self.lookup_general(&parameter.name) else {
                return Ok(false);
            };
            homes.push((parameter.name.clone(), register));
        }
        let home_of = |homes: &[(String, u8)], name: &str| {
            homes.iter().find(|(n, _)| n == name).map(|&(_, r)| r)
        };
        // The init list: `a = C`, `a = param`, or `a = param << K` — a
        // param read by its LAST init use frees its home.
        enum Init {
            Constant { register: u8, value: i16 },
            ShiftOfParam { register: u8, source: u8, amount: u8 },
        }
        let mut init_plan: Vec<Init> = Vec::new();
        for (name, constant) in &local_constant_inits {
            let top = homes.iter().map(|&(_, r)| r).filter(|&r| r != 0).max().unwrap_or(2);
            let register = top + 1;
            init_plan.push(Init::Constant { register, value: *constant });
            homes.push((name.to_string(), register));
        }
        if let Some(init) = initializer {
            // Flatten the comma list.
            let mut elements: Vec<&Expression> = Vec::new();
            let mut cursor = init;
            loop {
                match cursor {
                    Expression::Comma { left, right } => {
                        elements.push(right.as_ref());
                        cursor = left.as_ref();
                    }
                    other => {
                        elements.push(other);
                        break;
                    }
                }
            }
            elements.reverse();
            // First pass: aliases (i = param) rename in place; param-reads
            // mark freed homes.
            let mut freed: Vec<u8> = Vec::new();
            let mut pending: Vec<(&str, &Expression)> = Vec::new();
            for element in &elements {
                let Expression::Assign { target, value } = element else {
                    return Ok(false);
                };
                let Expression::Variable(name) = target.as_ref() else {
                    return Ok(false);
                };
                match value.as_ref() {
                    Expression::Variable(source) => {
                        // An alias: the local IS the param, renamed.
                        let Some(register) = home_of(&homes, source) else {
                            return Ok(false);
                        };
                        homes.push((name.clone(), register));
                    }
                    Expression::Binary { operator: BinaryOperator::ShiftLeft, left, right } => {
                        let Expression::Variable(source) = left.as_ref() else {
                            return Ok(false);
                        };
                        let Some(source_register) = home_of(&homes, source) else {
                            return Ok(false);
                        };
                        let Some(amount) =
                            crate::analysis::constant_value(right).and_then(|k| u8::try_from(k).ok())
                        else {
                            return Ok(false);
                        };
                        // A condition-only computed value lives in r0.
                        init_plan.push(Init::ShiftOfParam {
                            register: 0,
                            source: source_register,
                            amount,
                        });
                        homes.push((name.clone(), 0));
                        freed.push(source_register);
                    }
                    other if crate::analysis::constant_value(other).is_some() => {
                        pending.push((name.as_str(), other));
                    }
                    _ => return Ok(false),
                }
            }
            // Second pass: constants take a freed param home, else the next
            // free register after the params.
            for (name, value) in pending {
                let constant = crate::analysis::constant_value(value).expect("checked");
                let Ok(small) = i16::try_from(constant) else {
                    return Ok(false);
                };
                let register = if let Some(register) = freed.pop() {
                    register
                } else {
                    let top = homes.iter().map(|&(_, r)| r).filter(|&r| r != 0).max().unwrap_or(2);
                    top + 1
                };
                init_plan.push(Init::Constant { register, value: small });
                homes.push((name.to_string(), register));
            }
        }
        // The condition: var OP const, var OP var, or the char-walk
        // truthiness `*p` (lbz + extsb. record test).
        enum LoopTest {
            Constant { register: u8, constant: i64, big: bool },
            Register { left: u8, right: u8 },
            CharLoad { pointer: u8 },
        }
        let (loop_test, back_branch) = match condition {
            Expression::Binary { operator: cond_op, left: cond_left, right: cond_right } => {
                let Expression::Variable(cond_var) = cond_left.as_ref() else {
                    return Ok(false);
                };
                let Some(cond_register) = home_of(&homes, cond_var) else {
                    return Ok(false);
                };
                let back = match cond_op {
                    BinaryOperator::Greater => (12u8, 1u8), // bgt
                    BinaryOperator::Less => (12u8, 0u8),    // blt
                    _ => return Ok(false),
                };
                if let Some(constant) = crate::analysis::constant_value(cond_right) {
                    let big = i16::try_from(constant).is_err();
                    if big && constant & 0xffff != 0 {
                        return Ok(false); // lis-only bounds measured
                    }
                    (LoopTest::Constant { register: cond_register, constant, big }, back)
                } else if let Expression::Variable(right_var) = cond_right.as_ref() {
                    let Some(right_register) = home_of(&homes, right_var) else {
                        return Ok(false);
                    };
                    (LoopTest::Register { left: cond_register, right: right_register }, back)
                } else {
                    return Ok(false);
                }
            }
            Expression::Dereference { pointer } => {
                let Expression::Variable(name) = pointer.as_ref() else {
                    return Ok(false);
                };
                if !char_pointers.iter().any(|p| p == name) {
                    return Ok(false);
                }
                let Some(register) = home_of(&homes, name) else {
                    return Ok(false);
                };
                (LoopTest::CharLoad { pointer: register }, (4u8, 2u8)) // bne
            }
            _ => return Ok(false),
        };
        let hoists_big_bound = matches!(&loop_test, LoopTest::Constant { big: true, .. });
        // Body + step ops: compound self-ops on homed locals.
        enum LoopOp {
            AddImmediate { register: u8, value: i16 },
            SelfAdd { register: u8 },
            ShiftLeft { register: u8, amount: u8 },
            /// `*dst = *src` where src is the walk's condition pointer —
            /// the char loaded by the TEST carries across the back edge
            /// into this store (measured S2).
            CarriedStore { destination: u8 },
        }
        let condition_pointer: Option<&str> = match condition {
            Expression::Dereference { pointer } => match pointer.as_ref() {
                Expression::Variable(name) => Some(name.as_str()),
                _ => None,
            },
            _ => None,
        };
        let parse_op = |homes: &[(String, u8)], statement: &Statement| -> Option<LoopOp> {
            if let Statement::Store { target, value } = statement {
                // `*dst = *src` with src the condition pointer.
                let Expression::Dereference { pointer: dst } = target else { return None };
                let Expression::Variable(dst_name) = dst.as_ref() else { return None };
                let destination = home_of(homes, dst_name)?;
                let Expression::Dereference { pointer: src } = value else { return None };
                let Expression::Variable(src_name) = src.as_ref() else { return None };
                if condition_pointer != Some(src_name.as_str()) {
                    return None;
                }
                return Some(LoopOp::CarriedStore { destination });
            }
            let Statement::Assign { name, value } = statement else { return None };
            let register = home_of(homes, name)?;
            let Expression::Binary { operator, left, right } = value else { return None };
            if !matches!(left.as_ref(), Expression::Variable(v) if v == name) {
                return None;
            }
            match operator {
                BinaryOperator::Add if matches!(right.as_ref(), Expression::Variable(v) if v == name) => {
                    Some(LoopOp::SelfAdd { register })
                }
                BinaryOperator::Add => {
                    let value = i16::try_from(crate::analysis::constant_value(right)?).ok()?;
                    Some(LoopOp::AddImmediate { register, value })
                }
                BinaryOperator::Subtract => {
                    let value = i16::try_from(-crate::analysis::constant_value(right)?).ok()?;
                    Some(LoopOp::AddImmediate { register, value })
                }
                BinaryOperator::ShiftLeft => {
                    let amount = u8::try_from(crate::analysis::constant_value(right)?).ok()?;
                    Some(LoopOp::ShiftLeft { register, amount })
                }
                _ => None,
            }
        };
        // Loop ops in emission order: the STEP first (it feeds the
        // condition — measured), then the body statements in source order.
        let mut loop_ops: Vec<LoopOp> = Vec::new();
        match kind {
            LoopKind::For => {
                let Some(step) = step else { return Ok(false) };
                let step_statement = Statement::Assign {
                    name: match step {
                        Expression::Assign { target, .. } => match target.as_ref() {
                            Expression::Variable(name) => name.clone(),
                            _ => return Ok(false),
                        },
                        _ => return Ok(false),
                    },
                    value: match step {
                        Expression::Assign { value, .. } => value.as_ref().clone(),
                        _ => return Ok(false),
                    },
                };
                let Some(op) = parse_op(&homes, &step_statement) else {
                    return Ok(false);
                };
                loop_ops.push(op);
            }
            LoopKind::While => {
                if step.is_some() {
                    return Ok(false);
                }
            }
            LoopKind::DoWhile => {
                if step.is_some() {
                    return Ok(false);
                }
            }
        }
        for statement in body {
            let Some(op) = parse_op(&homes, statement) else {
                return Ok(false);
            };
            loop_ops.push(op);
        }
        if loop_ops.is_empty() {
            return Ok(false);
        }
        // COUNTED loops (the condition variable stepped by a constant in a
        // For/While) take mwcc's unroll machinery — claiming them rotated
        // would be WRONG BYTES. Only the do-while keeps constant steps
        // (measured D1: no unroll).
        if !matches!(kind, LoopKind::DoWhile) {
            let condition_variable_register = match &loop_test {
                LoopTest::Constant { register, .. } => Some(*register),
                LoopTest::Register { left, .. } => Some(*left),
                LoopTest::CharLoad { .. } => None,
            };
            if let Some(register) = condition_variable_register {
                let stepped_by_constant = loop_ops.iter().any(|op| {
                    matches!(op, LoopOp::AddImmediate { register: stepped, .. } if *stepped == register)
                });
                if stepped_by_constant {
                    return Ok(false);
                }
            }
        }
        let has_carried_store = loop_ops.iter().any(|op| matches!(op, LoopOp::CarriedStore { .. }));
        // The carried char takes the next free register (S2: r5).
        let carry_register = if has_carried_store {
            let top = homes.iter().map(|&(_, r)| r).filter(|&r| r != 0).max().unwrap_or(2);
            top + 1
        } else {
            0
        };
        let return_register = if function.return_type == Type::Void {
            3 // no move needed
        } else {
            let Some(Expression::Variable(returned)) = &function.return_expression else {
                return Ok(false);
            };
            let Some(register) = home_of(&homes, returned) else {
                return Ok(false);
            };
            register
        };
        // -- emit --
        // Init: param-reading shifts first, then constants (the freed-home
        // order); a big bound hoists to r0 before the loop.
        for init in &init_plan {
            match init {
                Init::ShiftOfParam { register, source, amount } => {
                    self.output.instructions.push(Instruction::ShiftLeftImmediate {
                        a: *register,
                        s: *source,
                        shift: *amount,
                    });
                }
                Init::Constant { register, value } => {
                    self.output.instructions.push(Instruction::load_immediate(*register, *value));
                }
            }
        }
        if let LoopTest::Constant { constant, big: true, .. } = &loop_test {
            self.output
                .instructions
                .push(Instruction::load_immediate_shifted(0, (constant >> 16) as i16));
        }
        let _ = hoists_big_bound;
        let body_at = self.fresh_label();
        let test_at = self.fresh_label();
        if !matches!(kind, LoopKind::DoWhile) {
            // The rotated entry; a do-while falls straight into its body.
            self.emit_branch_to(test_at);
        }
        self.bind_label(body_at);
        for op in &loop_ops {
            match op {
                LoopOp::AddImmediate { register, value } => {
                    self.output.instructions.push(Instruction::AddImmediate {
                        d: *register,
                        a: *register,
                        immediate: *value,
                    });
                }
                LoopOp::SelfAdd { register } => {
                    self.output.instructions.push(Instruction::Add {
                        d: *register,
                        a: *register,
                        b: *register,
                    });
                }
                LoopOp::ShiftLeft { register, amount } => {
                    self.output.instructions.push(Instruction::ShiftLeftImmediate {
                        a: *register,
                        s: *register,
                        shift: *amount,
                    });
                }
                LoopOp::CarriedStore { destination } => {
                    self.output.instructions.push(Instruction::StoreByte {
                        s: carry_register,
                        a: *destination,
                        offset: 0,
                    });
                }
            }
        }
        self.bind_label(test_at);
        match &loop_test {
            LoopTest::Constant { register, constant, big: true } => {
                let _ = constant;
                self.output.instructions.push(Instruction::CompareWord { a: *register, b: 0 });
            }
            LoopTest::Constant { register, constant, big: false } => {
                self.output.instructions.push(Instruction::CompareWordImmediate {
                    a: *register,
                    immediate: *constant as i16,
                });
            }
            LoopTest::Register { left, right } => {
                self.output.instructions.push(Instruction::CompareWord { a: *left, b: *right });
            }
            LoopTest::CharLoad { pointer } => {
                // A carried store loads into its carry register; a bare
                // walk uses r0.
                let target = if has_carried_store { carry_register } else { 0 };
                self.output.instructions.push(Instruction::LoadByteZero {
                    d: target,
                    a: *pointer,
                    offset: 0,
                });
                self.output.instructions.push(Instruction::ExtendSignByteRecord { a: 0, s: target });
            }
        }
        self.emit_branch_conditional_to(back_branch.0, back_branch.1, body_at);
        if function.return_type != Type::Void && return_register != 3 {
            self.output.instructions.push(Instruction::move_register(3, return_register));
        }
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // @N: measured after implementation (objprobe) — placeholder 0.
        self.output.anonymous_label_bump += 0;
        Ok(true)
    }

    /// The FPCLASSIFY SWITCH (fire 411, fminmaxdim's __fpclassifyd): a
    /// two-case-plus-default switch on `pun(x) & BIGMASK` whose arms are
    /// short-circuit || diamonds over the pun words. Measured: hx loads
    /// to r4 (live through the arms), the scrutinee rlwinm to r3, the
    /// tree compares r3 against the lis-built big value (cmpw) then 0
    /// (cmpwi); each arm: clrlwi. (record) -> bne TRUE; lwz the LOW word
    /// from the SPILL; cmpwi; beq FALSE; li/b-END per side; default li.
    fn try_fpclassify_switch(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::{ArmBody, Statement};
        if function.return_type != Type::Int
            || !function.guards.is_empty()
            || !function.locals.is_empty()
        {
            return Ok(false);
        }
        let [x_param] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if x_param.parameter_type != Type::Double {
            return Ok(false);
        }
        let x = x_param.name.as_str();
        // The default is either `default:` inside the switch, or the
        // trailing `return K;` after it (the real fminmaxdim form).
        let (scrutinee, arms, default) = match function.statements.as_slice() {
            [Statement::Switch { scrutinee, arms, default }]
                if function.return_expression.is_none() && default.is_some() =>
            {
                (scrutinee, arms, default.as_ref())
            }
            [Statement::Switch { scrutinee, arms, default }]
                if default.is_none() && function.return_expression.is_some() =>
            {
                (scrutinee, arms, function.return_expression.as_ref())
            }
            _ => return Ok(false),
        };
        // pun0(x) & BIGMASK (lis-only mask).
        let Expression::Binary { operator: BinaryOperator::BitAnd, left, right } = scrutinee else {
            return Ok(false);
        };
        if crate::frame::pun_word_offset_pub(left, x) != Some(0) {
            return Ok(false);
        }
        let Some(big_mask) = crate::analysis::constant_value(right) else {
            return Ok(false);
        };
        let big_mask = big_mask as u32 as i64;
        let Some((mask_begin, mask_end)) = crate::analysis::rlwinm_mask(big_mask) else {
            return Ok(false);
        };
        if big_mask & 0xffff != 0 {
            return Ok(false);
        }
        // Exactly two cases: the mask value itself + zero, and a default.
        let Some(default_value) = default else {
            return Ok(false);
        };
        let _ = &default_value;
        let Some(default_constant) =
            crate::analysis::constant_value(default_value).and_then(|k| i16::try_from(k).ok())
        else {
            return Ok(false);
        };
        if arms.len() != 2 {
            return Ok(false);
        }
        // The || diamond: (pun0 & M2) || pun4[& 0xffffffff] -> (A, B).
        struct Diamond {
            second_begin: u8,
            second_end: u8,
            when_true: i16,
            when_false: i16,
        }
        let parse_diamond = |body: &ArmBody| -> Option<Diamond> {
            let ArmBody::Statements(statements) = body else { return None };
            let [Statement::If { condition, then_body, else_body }] = statements.as_slice() else {
                return None;
            };
            let Expression::Binary { operator: BinaryOperator::LogicalOr, left, right } = condition
            else {
                return None;
            };
            let Expression::Binary { operator: BinaryOperator::BitAnd, left: p0, right: m2 } =
                left.as_ref()
            else {
                return None;
            };
            if crate::frame::pun_word_offset_pub(p0, x) != Some(0) {
                return None;
            }
            let (second_begin, second_end) =
                crate::analysis::rlwinm_mask(crate::analysis::constant_value(m2)?)?;
            // The low word, optionally masked with the identity 0xffffffff.
            let low_ok = match right.as_ref() {
                Expression::Binary { operator: BinaryOperator::BitAnd, left: p4, right: identity } => {
                    crate::frame::pun_word_offset_pub(p4, x) == Some(4)
                        && crate::analysis::constant_value(identity).map(|c| c as u32)
                            == Some(0xffff_ffff)
                }
                other => crate::frame::pun_word_offset_pub(other, x) == Some(4),
            };
            if !low_ok {
                return None;
            }
            let value_of = |body: &[Statement]| -> Option<i16> {
                let [Statement::Return(Some(value))] = body else { return None };
                crate::analysis::constant_value(value).and_then(|k| i16::try_from(k).ok())
            };
            Some(Diamond {
                second_begin,
                second_end,
                when_true: value_of(then_body)?,
                when_false: value_of(else_body)?,
            })
        };
        let mut big_arm: Option<Diamond> = None;
        let mut zero_arm: Option<Diamond> = None;
        for arm in arms {
            let diamond = parse_diamond(&arm.body);
            if arm.value == big_mask {
                big_arm = diamond;
            } else if arm.value == 0 {
                zero_arm = diamond;
            } else {
                return Ok(false);
            }
        }
        let (Some(big_arm), Some(zero_arm)) = (big_arm, zero_arm) else {
            return Ok(false);
        };
        // -- emit --
        self.frame_size = 16;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::load_immediate_shifted(0, (big_mask >> 16) as i16));
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::LoadWord { d: 4, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 3,
            s: 4,
            shift: 0,
            begin: mask_begin,
            end: mask_end,
        });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 0 });
        let big_at = self.fresh_label();
        let zero_at = self.fresh_label();
        let default_at = self.fresh_label();
        let end_at = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, big_at); // beq
        self.emit_branch_conditional_to(4, 0, default_at); // bge
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 3, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, zero_at); // beq
        self.emit_branch_to(default_at);
        let mut emit_diamond = |generator: &mut Self, diamond: &Diamond, label| {
            generator.bind_label(label);
            let when_true = generator.fresh_label();
            let when_false = generator.fresh_label();
            generator.output.instructions.push(Instruction::AndMaskRecord {
                a: 0,
                s: 4,
                begin: diamond.second_begin,
                end: diamond.second_end,
            });
            generator.emit_branch_conditional_to(4, 2, when_true); // bne
            generator.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 12 });
            generator.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
            generator.emit_branch_conditional_to(12, 2, when_false); // beq
            generator.bind_label(when_true);
            generator.output.instructions.push(Instruction::load_immediate(3, diamond.when_true));
            generator.emit_branch_to(end_at);
            generator.bind_label(when_false);
            generator.output.instructions.push(Instruction::load_immediate(3, diamond.when_false));
            generator.emit_branch_to(end_at);
        };
        emit_diamond(self, &big_arm, big_at);
        emit_diamond(self, &zero_arm, zero_at);
        self.bind_label(default_at);
        self.output.instructions.push(Instruction::load_immediate(3, default_constant));
        self.bind_label(end_at);
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // Pre-pool labels (measured @18 on the fpclassify object vs the
        // +0 base's @5).
        self.output.anonymous_label_bump += 13;
        Ok(true)
    }

    /// The TRIG DISPATCHER (fire 408, s_sin/s_cos): the fdlibm range
    /// dispatch — a small-|x| kernel call, the inf/NaN x-x rung, then
    /// __ieee754_rem_pio2 into a frame array and a four-way switch of
    /// kernel calls (negated for quadrants 2/3). Measured: frame 32
    /// (x spill 8, y[2] at 16), the K1 synthesis in the mflr latency
    /// slot, cmpw REGISTER compares against lis-built bounds, the
    /// binary switch tree [cmpwi 1: beq C1 | bge -> cmpwi 3: bge DEF,
    /// b C2 | cmpwi 0: bge C0, b DEF], per-arm lfd/li/lfd argument
    /// loads, and fneg on the negated results.
    fn try_trig_dispatcher(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        if function.return_type != Type::Double || !function.guards.is_empty() {
            return Ok(false);
        }
        let [x_param] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if x_param.parameter_type != Type::Double {
            return Ok(false);
        }
        let x = x_param.name.as_str();
        // Locals: y[2] (double array), z = 0.0, and the two ints.
        let mut array: Option<&str> = None;
        let mut zero_local: Option<&str> = None;
        let mut ints: Vec<&str> = Vec::new();
        for local in &function.locals {
            match (local.declared_type, local.array_length) {
                (Type::Double, Some(2)) if array.is_none() => array = Some(local.name.as_str()),
                (Type::Double, None)
                    if matches!(&local.initializer, Some(Expression::FloatLiteral(z)) if *z == 0.0)
                        && zero_local.is_none() =>
                {
                    zero_local = Some(local.name.as_str())
                }
                (Type::Int, None) if local.initializer.is_none() => ints.push(local.name.as_str()),
                _ => return Ok(false),
            }
        }
        let (Some(array), Some(zero_local)) = (array, zero_local) else {
            return Ok(false);
        };
        if ints.len() != 2 {
            return Ok(false);
        }
        // statements (the parser FLATTENS the else-if returns):
        // ix = pun(x); ix &= 0x7fffffff; if (ix<=K1) return call;
        // if (ix>=K2) return x-x; n = rem(x,y); switch (n&3) {...}.
        // Two tails: the four-way SWITCH of kernels (sin/cos), or the
        // direct parity call `return kernel(y0,y1,1-((n&1)<<1))` (tan) —
        // the latter arrives as the function's trailing return.
        let (head, switch_tail, return_tail): (&[Statement], Option<(&Expression, &Vec<mwcc_syntax_trees::SwitchArm>, &Option<Expression>)>, Option<&Expression>) =
            match function.statements.as_slice() {
                [head @ .., Statement::Switch { scrutinee, arms, default }] if head.len() == 5 => {
                    (head, Some((scrutinee, arms, default)), None)
                }
                [head @ .., Statement::Return(Some(value))] if head.len() == 5 => {
                    (head, None, Some(value))
                }
                _ => return Ok(false),
            };
        let [Statement::Assign { name: ix1, value: pun }, Statement::Assign { name: ix2, value: mask }, Statement::If { condition: small, then_body: small_arm, else_body: small_else }, Statement::If { condition: huge_cond, then_body: huge_arm, else_body: huge_else }, Statement::Assign { name: n_name, value: rem_call }] =
            head
        else {
            return Ok(false);
        };
        if !small_else.is_empty() || !huge_else.is_empty() {
            return Ok(false);
        }
        let ix = ix1.as_str();
        if ix2 != ix
            || !ints.contains(&ix)
            || crate::frame::pun_word_offset_pub(pun, x) != Some(0)
            || !matches!(mask, Expression::Binary { operator: BinaryOperator::BitAnd, left, right }
                if matches!(left.as_ref(), Expression::Variable(v) if v == ix)
                    && crate::analysis::constant_value(right) == Some(0x7fff_ffff))
        {
            return Ok(false);
        }
        // if (ix <= K1) return kernel(x, z, 0);
        let Expression::Binary { operator: BinaryOperator::LessEqual, left, right } = small else {
            return Ok(false);
        };
        if !matches!(left.as_ref(), Expression::Variable(v) if v == ix) {
            return Ok(false);
        }
        let Some(k1) = crate::analysis::constant_value(right) else {
            return Ok(false);
        };
        let [Statement::Return(Some(Expression::Call { name: small_callee, arguments: small_args }))] =
            small_arm.as_slice()
        else {
            return Ok(false);
        };
        // kernel(x, z, 0) or kernel(x, z) — the int arg optional (cos).
        let small_int = match small_args.as_slice() {
            [Expression::Variable(a), Expression::Variable(z)] if a == x && z == zero_local => None,
            [Expression::Variable(a), Expression::Variable(z), n]
                if a == x && z == zero_local && crate::analysis::constant_value(n).is_some() =>
            {
                Some(crate::analysis::constant_value(n).expect("checked") as i16)
            }
            _ => return Ok(false),
        };
        // if (ix >= K2) return x - x;
        let Expression::Binary { operator: BinaryOperator::GreaterEqual, left, right } = huge_cond
        else {
            return Ok(false);
        };
        if !matches!(left.as_ref(), Expression::Variable(v) if v == ix) {
            return Ok(false);
        }
        let Some(k2) = crate::analysis::constant_value(right) else {
            return Ok(false);
        };
        if k1 & 0xffff == 0 || k2 & 0xffff != 0 {
            // K1 synthesizes lis+addi; K2 is lis-only (measured shapes).
            return Ok(false);
        }
        if !matches!(huge_arm.as_slice(), [Statement::Return(Some(Expression::Binary { operator: BinaryOperator::Subtract, left, right }))]
            if matches!(left.as_ref(), Expression::Variable(v) if v == x)
                && matches!(right.as_ref(), Expression::Variable(v) if v == x))
        {
            return Ok(false);
        }
        // n = rem_pio2(x, y); switch (n & 3) { ... }
        if !ints.contains(&n_name.as_str()) || n_name == ix {
            return Ok(false);
        }
        let Expression::Call { name: rem_callee, arguments: rem_args } = rem_call else {
            return Ok(false);
        };
        if !matches!(rem_args.as_slice(), [Expression::Variable(a), Expression::Variable(y)]
            if a == x && y == array)
        {
            return Ok(false);
        }
        // The tan tail: return kernel(y[0], y[1], 1 - ((n & 1) << 1)).
        let parity_tail: Option<String> = if switch_tail.is_none() {
            let Some(Expression::Call { name, arguments }) = return_tail else {
                return Ok(false);
            };
            let ok = matches!(arguments.as_slice(),
                [Expression::Index { base, index: i0 }, Expression::Index { base: b1, index: i1 }, parity]
                    if matches!(base.as_ref(), Expression::Variable(v) if v == array)
                        && matches!(b1.as_ref(), Expression::Variable(v) if v == array)
                        && crate::analysis::constant_value(i0) == Some(0)
                        && crate::analysis::constant_value(i1) == Some(1)
                        && matches!(parity, Expression::Binary { operator: BinaryOperator::Subtract, left: one, right: shifted }
                            if crate::analysis::constant_value(one) == Some(1)
                                && matches!(shifted.as_ref(), Expression::Binary { operator: BinaryOperator::ShiftLeft, left: masked, right: by_one }
                                    if crate::analysis::constant_value(by_one) == Some(1)
                                        && matches!(masked.as_ref(), Expression::Binary { operator: BinaryOperator::BitAnd, left: nv, right: m1 }
                                            if matches!(nv.as_ref(), Expression::Variable(v) if v == n_name.as_str())
                                                && crate::analysis::constant_value(m1) == Some(1)))));
            if !ok {
                return Ok(false);
            }
            Some(name.clone())
        } else {
            None
        };
        if let Some((scrutinee, _, _)) = &switch_tail {
            if !matches!(*scrutinee, Expression::Binary { operator: BinaryOperator::BitAnd, left, right }
                if matches!(left.as_ref(), Expression::Variable(v) if v == n_name.as_str())
                    && crate::analysis::constant_value(right) == Some(3))
            {
                return Ok(false);
            }
        }
        // The four arms: (callee, int arg, negated) per quadrant 0..3.
        struct Quadrant {
            callee: String,
            int_argument: Option<i16>,
            negated: bool,
        }
        let parse_quadrant = |result: &Expression| -> Option<Quadrant> {
            let (call, negated) = match result {
                Expression::Unary { operator: UnaryOperator::Negate, operand } => {
                    (operand.as_ref(), true)
                }
                other => (other, false),
            };
            let Expression::Call { name, arguments } = call else { return None };
            let int_argument = match arguments.as_slice() {
                [Expression::Index { base, index: i0 }, Expression::Index { base: b1, index: i1 }]
                    if matches!(base.as_ref(), Expression::Variable(v) if v == array)
                        && matches!(b1.as_ref(), Expression::Variable(v) if v == array)
                        && crate::analysis::constant_value(i0) == Some(0)
                        && crate::analysis::constant_value(i1) == Some(1) =>
                {
                    None
                }
                [Expression::Index { base, index: i0 }, Expression::Index { base: b1, index: i1 }, n]
                    if matches!(base.as_ref(), Expression::Variable(v) if v == array)
                        && matches!(b1.as_ref(), Expression::Variable(v) if v == array)
                        && crate::analysis::constant_value(i0) == Some(0)
                        && crate::analysis::constant_value(i1) == Some(1)
                        && crate::analysis::constant_value(n).is_some() =>
                {
                    Some(crate::analysis::constant_value(n).expect("checked") as i16)
                }
                _ => return None,
            };
            Some(Quadrant { callee: name.clone(), int_argument, negated })
        };
        let mut quadrants: Vec<Option<Quadrant>> = vec![None, None, None, None];
        if let Some((_, arms, default)) = &switch_tail {
            for arm in arms.iter() {
                let index = arm.value;
                if !(0..3).contains(&index) {
                    return Ok(false);
                }
                let Some(result) = arm.result() else {
                    return Ok(false);
                };
                quadrants[index as usize] = parse_quadrant(result);
            }
            let Some(default_result) = default else {
                return Ok(false);
            };
            quadrants[3] = parse_quadrant(default_result);
            if quadrants.iter().any(|quadrant| quadrant.is_none()) {
                return Ok(false);
            }
        }
        // -- emit --
        self.non_leaf = true;
        self.frame_size = 32;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -32 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        // K1's lis fills the mflr latency slot.
        self.output.instructions.push(Instruction::load_immediate_shifted(3, ((k1 + 0x8000) >> 16) as i16));
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 1, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::AddImmediate { d: 0, a: 3, immediate: k1 as i16 });
        self.output.instructions.push(Instruction::LoadWord { d: 3, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::RotateAndMask { a: 3, s: 3, shift: 0, begin: 1, end: 31 });
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 0 });
        let epilogue = self.fresh_label();
        let huge_at = self.fresh_label();
        self.emit_branch_conditional_to(12, 1, huge_at); // bgt
        // The small arm: kernel(x, z, 0).
        self.load_double_constant(2, 0.0f64.to_bits());
        if let Some(int_argument) = small_int {
            self.output.instructions.push(Instruction::load_immediate(3, int_argument));
        }
        self.record_relocation(RelocationKind::Rel24, small_callee);
        self.output.instructions.push(Instruction::BranchAndLink { target: small_callee.clone() });
        self.emit_branch_to(epilogue);
        // else if (ix >= K2) return x - x;
        self.bind_label(huge_at);
        self.output.instructions.push(Instruction::load_immediate_shifted(0, (k2 >> 16) as i16));
        self.output.instructions.push(Instruction::CompareWord { a: 3, b: 0 });
        let rem_at = self.fresh_label();
        self.emit_branch_conditional_to(12, 0, rem_at); // blt
        self.output.instructions.push(Instruction::FloatSubtractDouble { d: 1, a: 1, b: 1 });
        self.emit_branch_to(epilogue);
        // n = rem_pio2(x, &y); the switch tree.
        self.bind_label(rem_at);
        self.output.instructions.push(Instruction::AddImmediate { d: 3, a: 1, immediate: 16 });
        self.record_relocation(RelocationKind::Rel24, rem_callee);
        self.output.instructions.push(Instruction::BranchAndLink { target: rem_callee.clone() });
        if let Some(parity_callee) = &parity_tail {
            // tan: rlwinm r0,r3,1,30,30 ((n&1)<<1 fused); lfd f1/f2;
            // subfic r3,r0,1 between the loads and the call; fall to EPI.
            self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 3, shift: 1, begin: 30, end: 30 });
            self.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 16 });
            self.output.instructions.push(Instruction::LoadFloatDouble { d: 2, a: 1, offset: 24 });
            self.output.instructions.push(Instruction::SubtractFromImmediate { d: 3, a: 0, immediate: 1 });
            self.record_relocation(RelocationKind::Rel24, parity_callee);
            self.output.instructions.push(Instruction::BranchAndLink { target: parity_callee.clone() });
            self.bind_label(epilogue);
            self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 36 });
            self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
            self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 32 });
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            // Pre-pool labels (measure via objprobe on the tan object).
            self.output.anonymous_label_bump += 8;
            return Ok(true);
        }
        self.output.instructions.push(Instruction::RotateAndMask { a: 0, s: 3, shift: 0, begin: 30, end: 31 });
        let case0 = self.fresh_label();
        let case1 = self.fresh_label();
        let case2 = self.fresh_label();
        let case3 = self.fresh_label();
        let mid = self.fresh_label();
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, case1); // beq
        self.emit_branch_conditional_to(4, 0, mid); // bge -> the 2/3 side
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(4, 0, case0); // bge
        self.emit_branch_to(case3);
        self.bind_label(mid);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: 0, immediate: 3 });
        self.emit_branch_conditional_to(4, 0, case3); // bge
        self.emit_branch_to(case2);
        // The arms.
        let mut emit_arm = |generator: &mut Self, quadrant: &Quadrant, label, falls: bool| {
            generator.bind_label(label);
            generator.output.instructions.push(Instruction::LoadFloatDouble { d: 1, a: 1, offset: 16 });
            if let Some(int_argument) = quadrant.int_argument {
                generator.output.instructions.push(Instruction::load_immediate(3, int_argument));
            }
            generator.output.instructions.push(Instruction::LoadFloatDouble { d: 2, a: 1, offset: 24 });
            generator.record_relocation(RelocationKind::Rel24, &quadrant.callee);
            generator
                .output
                .instructions
                .push(Instruction::BranchAndLink { target: quadrant.callee.clone() });
            if quadrant.negated {
                generator.output.instructions.push(Instruction::FloatNegate { d: 1, b: 1 });
            }
            if !falls {
                generator.emit_branch_to(epilogue);
            }
        };
        let [Some(q0), Some(q1), Some(q2), Some(q3)] = &quadrants[..] else {
            unreachable!("validated above");
        };
        emit_arm(self, q0, case0, false);
        emit_arm(self, q1, case1, false);
        emit_arm(self, q2, case2, false);
        emit_arm(self, q3, case3, true);
        self.bind_label(epilogue);
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 36 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 32 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // Pre-pool labels (measured @18 on the s_sin object vs the +0
        // base's @5).
        self.output.anonymous_label_bump += 13;
        Ok(true)
    }

    /// The __frsqrte NEWTON SQRT (fire 407, the Dolphin math_inlines
    /// pattern): a LEAF float ladder around N reciprocal-sqrt refinement
    /// steps. Measured: lfd 0.0; fcmpo; ble; frsqrte f2,f1; lfd f4(.5);
    /// lfd f3(3.0); N x [fmul f0,f2,f2; fmul f2,f4,f2; fnmsub f0,f1,f0,f3;
    /// fmul f2,f2,f0] with the LAST step's product landing in f0; fmul
    /// f1,f1,f0; blr — then the ladder: fcmpu f0,f1 (==0, operands
    /// pool-first); fmr f1,f0; fcmpu f1,f0 (bare x, swapped); lis+lfs
    /// through the NAN/INFINITY int-array globals (Addr16 pairs).
    fn try_frsqrte_sqrt(&mut self, function: &Function) -> Compilation<bool> {
        use mwcc_syntax_trees::Statement;
        if function.return_type != Type::Double || !function.guards.is_empty() {
            return Ok(false);
        }
        let [x_param] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if x_param.parameter_type != Type::Double {
            return Ok(false);
        }
        let x = x_param.name.as_str();
        let [guess_local] = function.locals.as_slice() else {
            return Ok(false);
        };
        if guess_local.declared_type != Type::Double || guess_local.initializer.is_some() {
            return Ok(false);
        }
        let guess = guess_local.name.as_str();
        let [Statement::If { condition, then_body, else_body }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        // if (x > 0.0)
        if !matches!(condition, Expression::Binary { operator: BinaryOperator::Greater, left, right }
            if matches!(left.as_ref(), Expression::Variable(v) if v == x)
                && matches!(right.as_ref(), Expression::FloatLiteral(zero) if *zero == 0.0))
        {
            return Ok(false);
        }
        // then: guess = __frsqrte(x); N refinements; return x * guess.
        let [Statement::Assign { name: seed_name, value: seed }, refinements @ .., Statement::Return(Some(product))] =
            then_body.as_slice()
        else {
            return Ok(false);
        };
        if seed_name != guess
            || !matches!(seed, Expression::Call { name, arguments }
                if name == "__frsqrte"
                    && matches!(arguments.as_slice(), [Expression::Variable(v)] if v == x))
        {
            return Ok(false);
        }
        if refinements.is_empty() {
            return Ok(false);
        }
        // Each: guess = .5 * guess * (3.0 - guess * guess * x)
        for refinement in refinements {
            let Statement::Assign { name, value } = refinement else {
                return Ok(false);
            };
            if name != guess {
                return Ok(false);
            }
            let ok = matches!(value, Expression::Binary { operator: BinaryOperator::Multiply, left, right }
                if matches!(left.as_ref(), Expression::Binary { operator: BinaryOperator::Multiply, left: half, right: g }
                    if matches!(half.as_ref(), Expression::FloatLiteral(h) if *h == 0.5)
                        && matches!(g.as_ref(), Expression::Variable(v) if v == guess))
                    && matches!(right.as_ref(), Expression::Binary { operator: BinaryOperator::Subtract, left: three, right: ggx }
                        if matches!(three.as_ref(), Expression::FloatLiteral(t) if *t == 3.0)
                            && matches!(ggx.as_ref(), Expression::Binary { operator: BinaryOperator::Multiply, left: gg, right: xv }
                                if matches!(xv.as_ref(), Expression::Variable(v) if v == x)
                                    && matches!(gg.as_ref(), Expression::Binary { operator: BinaryOperator::Multiply, left: g1, right: g2 }
                                        if matches!(g1.as_ref(), Expression::Variable(v) if v == guess)
                                            && matches!(g2.as_ref(), Expression::Variable(v) if v == guess)))));
            if !ok {
                return Ok(false);
            }
        }
        if !matches!(product, Expression::Binary { operator: BinaryOperator::Multiply, left, right }
            if matches!(left.as_ref(), Expression::Variable(v) if v == x)
                && matches!(right.as_ref(), Expression::Variable(v) if v == guess))
        {
            return Ok(false);
        }
        // else: if (x == 0.0) return 0; else if (x) return *(float*)NAN;
        // ... with the trailing return *(float*)INF.
        let [Statement::If { condition: zero_cond, then_body: zero_then, else_body: zero_else }] =
            else_body.as_slice()
        else {
            return Ok(false);
        };
        if !matches!(zero_cond, Expression::Binary { operator: BinaryOperator::Equal, left, right }
            if matches!(left.as_ref(), Expression::Variable(v) if v == x)
                && matches!(right.as_ref(), Expression::FloatLiteral(zero) if *zero == 0.0))
        {
            return Ok(false);
        }
        if !matches!(zero_then.as_slice(), [Statement::Return(Some(value))]
            if crate::analysis::constant_value(value) == Some(0))
        {
            return Ok(false);
        }
        let float_global = |expression: &Expression| -> Option<String> {
            let Expression::Dereference { pointer } = expression else { return None };
            let Expression::Cast { target_type: Type::Pointer(Pointee::Float), operand } =
                pointer.as_ref()
            else {
                return None;
            };
            let Expression::Variable(name) = operand.as_ref() else { return None };
            Some(name.clone())
        };
        let [Statement::If { condition: nan_cond, then_body: nan_then, else_body: nan_else }] =
            zero_else.as_slice()
        else {
            return Ok(false);
        };
        if !matches!(nan_cond, Expression::Variable(v) if v == x) || !nan_else.is_empty() {
            return Ok(false);
        }
        let [Statement::Return(Some(nan_value))] = nan_then.as_slice() else {
            return Ok(false);
        };
        let (Some(nan_symbol), Some(Some(infinity_symbol))) = (
            float_global(nan_value),
            function.return_expression.as_ref().map(|value| float_global(value)),
        ) else {
            return Ok(false);
        };
        // -- emit (a leaf: no frame at all) --
        let steps = refinements.len();
        self.load_double_constant(0, 0.0f64.to_bits());
        self.output.instructions.push(Instruction::FloatCompareOrdered { a: 1, b: 0 });
        let ladder = self.fresh_label();
        self.emit_branch_conditional_to(4, 1, ladder); // ble
        self.output.instructions.push(Instruction::FloatReciprocalSqrtEstimate { d: 2, b: 1 });
        self.load_double_constant(4, 0.5f64.to_bits());
        self.load_double_constant(3, 3.0f64.to_bits());
        for step in 0..steps {
            let last = step + 1 == steps;
            self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 0, a: 2, c: 2 });
            self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 2, a: 4, c: 2 });
            self.output.instructions.push(Instruction::FloatNegativeMultiplySubtractDouble {
                d: 0,
                a: 1,
                c: 0,
                b: 3,
            });
            self.output.instructions.push(Instruction::FloatMultiplyDouble {
                d: if last { 0 } else { 2 },
                a: 2,
                c: 0,
            });
        }
        self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 1, a: 1, c: 0 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(ladder);
        // x == 0.0: fcmpu with the POOL value first; return the pooled 0.
        self.output.instructions.push(Instruction::FloatCompareUnordered { a: 0, b: 1 });
        let nan_at = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, nan_at); // bne
        self.output.instructions.push(Instruction::FloatMove { d: 1, b: 0 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(nan_at);
        // bare x: fcmpu the other way; INFINITY on equal-to-zero.
        self.output.instructions.push(Instruction::FloatCompareUnordered { a: 1, b: 0 });
        let infinity_at = self.fresh_label();
        self.emit_branch_conditional_to(12, 2, infinity_at); // beq
        self.emit_address_high(3, &nan_symbol);
        self.record_relocation(RelocationKind::Addr16Lo, &nan_symbol);
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 1, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        self.bind_label(infinity_at);
        self.emit_address_high(3, &infinity_symbol);
        self.record_relocation(RelocationKind::Addr16Lo, &infinity_symbol);
        self.output.instructions.push(Instruction::LoadFloatSingle { d: 1, a: 3, offset: 0 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        // Pre-pool labels (measured @17 on the math_inlines object vs the
        // +0 base's @5).
        self.output.anonymous_label_bump += 12;
        Ok(true)
    }

    /// The FLOAT callee-saved survivor (fire 406, C1): `return g(x) OP x;`
    /// with a double parameter surviving one external call. Measured:
    /// stwu -16; mflr; stw r0,20; stfd f31,8; fmr f31,f1; bl; lwz r0,20
    /// (the LR reload FIRST); the op; lfd f31,8; mtlr; addi; blr. The
    /// fmr copy leaves f1 holding x for the call itself.
    fn try_float_callee_saved(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty()
            || !function.statements.is_empty()
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || function.return_type != Type::Double
        {
            return Ok(false);
        }
        let [x_param] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if x_param.parameter_type != Type::Double {
            return Ok(false);
        }
        let x = x_param.name.as_str();
        let Some(Expression::Binary { operator, left, right }) = &function.return_expression else {
            return Ok(false);
        };
        // Call OP x, or x OP call. The emitted op reads (f31 = x) and
        // (f1 = the result): mwcc commutes adds to put the saved value
        // first (measured fadd f1,f31,f1 for `g(x) + x`).
        let (call, call_first) = match (left.as_ref(), right.as_ref()) {
            (Expression::Call { name, arguments }, Expression::Variable(v)) if v == x => {
                ((name, arguments), true)
            }
            (Expression::Variable(v), Expression::Call { name, arguments }) if v == x => {
                ((name, arguments), false)
            }
            _ => return Ok(false),
        };
        let (callee, arguments) = call;
        // A single argument: x itself (the fmr leaves f1 intact).
        if !matches!(arguments.as_slice(), [Expression::Variable(v)] if v == x) {
            return Ok(false);
        }
        // The op: fadd (commuted saved-first), fsub per order, fmul
        // (commuted saved-first).
        enum Op {
            Add,
            Mul,
            SubCallMinusX,
            SubXMinusCall,
        }
        let op = match operator {
            BinaryOperator::Add => Op::Add,
            BinaryOperator::Multiply => Op::Mul,
            BinaryOperator::Subtract if call_first => Op::SubCallMinusX,
            BinaryOperator::Subtract => Op::SubXMinusCall,
            _ => return Ok(false),
        };
        // -- emit --
        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved_float = 1;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::StoreFloatDouble { s: 31, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::FloatMove { d: 31, b: 1 });
        self.record_relocation(RelocationKind::Rel24, callee);
        self.output.instructions.push(Instruction::BranchAndLink { target: callee.to_string() });
        // The MULTIPLY schedules ahead of the LR reload (its latency
        // starts early — measured); add/sub follow the reload.
        if matches!(op, Op::Mul) {
            self.output.instructions.push(Instruction::FloatMultiplyDouble { d: 1, a: 31, c: 1 });
            self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        } else {
            self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
            match op {
                Op::Add => self
                    .output
                    .instructions
                    .push(Instruction::FloatAddDouble { d: 1, a: 31, b: 1 }),
                Op::SubCallMinusX => self
                    .output
                    .instructions
                    .push(Instruction::FloatSubtractDouble { d: 1, a: 1, b: 31 }),
                Op::SubXMinusCall => self
                    .output
                    .instructions
                    .push(Instruction::FloatSubtractDouble { d: 1, a: 31, b: 1 }),
                Op::Mul => unreachable!(),
            }
        }
        self.output.instructions.push(Instruction::LoadFloatDouble { d: 31, a: 1, offset: 8 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    fn try_callee_saved(&mut self, function: &Function) -> Compilation<bool> {
        // Address-taken locals are handled by the frame-resident path before this.
        if !self.frame_slots.is_empty() {
            return Ok(false);
        }
        // The body is straight-line calls and stores (control flow routes through its own
        // paths). A trailing store sink (`foo(); gi = a;`) saves the live value, runs the
        // calls, then stores it from the callee-saved register; mwcc orders that epilogue's
        // LR reload before the GPR reload (epilogue_lr_first), unlike the return sink.
        if function.statements.iter().any(|statement| !matches!(statement, Statement::Expression(_) | Statement::Store { .. })) {
            return Ok(false);
        }
        let has_store = function.statements.iter().any(|statement| matches!(statement, Statement::Store { .. }));
        if matches!(function.return_type, Type::Float | Type::Double) {
            return Ok(false);
        }
        let Some(live) = values_live_across_call(function) else {
            return Ok(false);
        };
        if live.is_empty() {
            return Ok(false);
        }
        // Every live value must be a general-class parameter (locals defer), and none
        // may be passed to a call — the first such argument use stays in the incoming
        // register (mwcc skips the move until a call clobbers it), which needs
        // value-location tracking not modeled here.
        let passed_to_call = function.statements.iter().any(|statement| match statement {
            Statement::Expression(expression) => live.iter().any(|name| expression_reads_name(expression, name)),
            _ => false,
        });
        if passed_to_call {
            return Ok(false);
        }
        // (parameter index, name, incoming register) for each promoted value.
        let mut promoted: Vec<(usize, String, u8)> = Vec::new();
        for name in &live {
            let Some(index) = function.parameters.iter().position(|parameter| &parameter.name == name) else {
                return Ok(false);
            };
            let (class, incoming) = match self.locations.get(name) {
                Some(location) => (location.class, location.register),
                None => return Ok(false),
            };
            if class != ValueClass::General {
                return Ok(false);
            }
            promoted.push((index, name.clone(), incoming));
        }
        // Highest register (r31) to the last parameter, descending toward the first.
        promoted.sort_by_key(|(index, _, _)| *index);

        let count = promoted.len();
        // A store sink takes one or two saved values: the epilogue reloads all-but-the-
        // lowest GPR, then LR, then the lowest, matching mwcc for count 1 and 2 (three or
        // more reschedule the LR reload by register death — `lwz r31; lwz r30; lwz r29; lwz
        // r0` — and defer). A second saved value must be void; a value returned alongside
        // two saved values interleaves the return move with the epilogue and is not modeled.
        if has_store && (count > 2 || (count == 2 && function.return_type != Type::Void)) {
            return Ok(false);
        }
        // A two-value store sink stores the saved values directly (`gi = a; gj = b;`). A
        // computed store (`gi = a + 1;`) reschedules around the two saves/epilogue and is
        // deferred; the single-value sink still allows a computed store.
        if has_store
            && count == 2
            && !function.statements.iter().all(|statement| match statement {
                Statement::Store { value, .. } => matches!(value, Expression::Variable(_)),
                _ => true,
            })
        {
            return Ok(false);
        }
        // With more than one saved value RETURNED, mwcc's scheduler interleaves the
        // epilogue restores with the post-call computation by register death — which we
        // don't model yet. It coincides with "all restores after" only when the values
        // combine in a single low-latency instruction (`a+b`, `a-b`, `a&b`); two
        // values through a multiply, or three or more values (multi-step), reschedule
        // the restores. Restrict count > 1 to that one safe shape. (A two-value store sink
        // has its own epilogue order above, so it skips this return-shape gate.)
        if count >= 2 && !has_store {
            let single_op = matches!(
                function.return_expression.as_ref(),
                Some(Expression::Binary { operator, left, right })
                    if count == 2
                        && matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract
                            | BinaryOperator::BitAnd | BinaryOperator::BitOr | BinaryOperator::BitXor)
                        && matches!(left.as_ref(), Expression::Variable(_))
                        && matches!(right.as_ref(), Expression::Variable(_))
            );
            if !single_op {
                return Ok(false);
            }
        }
        let frame_size = (((8 + 4 * count as i32) + 15) / 16 * 16) as i16;
        self.non_leaf = true;
        self.frame_size = frame_size;
        // Phase D: the promoted parameters' homes are virtuals, created highest-rank
        // first — id order reproduces r31, r30, … through the callee-saved pool. The
        // interleaved save+move prologue comes from the FRAME BUILDER.
        let homes: Vec<u8> = (0..count).map(|_| self.fresh_virtual_general()).collect();
        self.callee_saved = homes.clone();
        // A store sink reloads the saved LR before the GPR reloads in the epilogue.
        self.epilogue_lr_first = has_store;
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        debug_assert_eq!(plan.frame_size, frame_size);
        let incoming_ordered: Vec<u8> = promoted.iter().rev().map(|(_, _, incoming)| *incoming).collect();
        self.output.instructions.extend(plan.prologue_interleaved(&incoming_ordered));
        for (rank, (_, name, _)) in promoted.iter().rev().enumerate() {
            if let Some(location) = self.locations.get_mut(name) {
                location.register = homes[rank];
            }
        }

        for statement in &function.statements {
            self.emit_statement(statement)?;
        }
        if function.return_type != Type::Void {
            let result = Eabi::general_result().number;
            let return_expression = function
                .return_expression
                .as_ref()
                .ok_or_else(|| Diagnostic::error("a non-void function needs a return value"))?;
            self.evaluate_tail(return_expression, function.return_type, result)?;
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// `void s(T *p, …) { *p = g(args); }` — a call's result stored through a pointer
    /// PARAMETER that must survive the call. mwcc saves the pointer in r31 (`mr r31,r3`),
    /// runs the call, then stores the result through r31 (`stw r3,0(r31)`); the store-sink
    /// epilogue reloads LR before r31. Restricted to a general (int/pointer/narrow) pointee,
    /// a general-returning call, and arguments that do not reference the saved pointer.
    fn try_store_call_through_pointer(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.locals.is_empty() {
            return Ok(false);
        }
        // Void, or a non-void function returning a CONSTANT — materialized in r3 after the
        // store, before the epilogue (`*p=g(); return 0;` -> `stw r3,0(r31); li r3,0; …`). A
        // non-constant return (`return *p` re-reads the saved pointer with an interleaved
        // epilogue; `return x` reads a call-clobbered parameter) defers.
        let returns_constant = function.return_type != Type::Void
            && matches!(function.return_type, Type::Int | Type::UnsignedInt)
            && function.return_expression.as_ref().map_or(false, |expression| constant_value(expression).is_some());
        if function.return_type != Type::Void && !returns_constant {
            return Ok(false);
        }
        let [Statement::Store { target, value }] = function.statements.as_slice() else {
            return Ok(false);
        };
        let Expression::Call { name, arguments } = value else { return Ok(false) };
        // A store target through a pointer PARAMETER: `*p`, `p[const]`, or `p->m` — each
        // resolves to (the pointer variable, a byte offset, the stored width's pointee).
        let (pointer_name, byte_offset, pointee): (&str, i64, Pointee) = match target {
            Expression::Dereference { pointer } => {
                let Expression::Variable(name) = pointer.as_ref() else { return Ok(false) };
                (name, 0, self.pointee_of(pointer)?)
            }
            Expression::Index { base, index } => {
                let Expression::Variable(name) = base.as_ref() else { return Ok(false) };
                let Some(constant) = constant_value(index) else { return Ok(false) };
                let pointee = self.pointee_of(base)?;
                (name, constant * pointee.size() as i64, pointee)
            }
            Expression::Member { base, offset, member_type, index_stride: None } => {
                let Expression::Variable(name) = base.as_ref() else { return Ok(false) };
                let Some(pointee) = pointee_of_type(*member_type) else { return Ok(false) };
                (name, *offset as i64, pointee)
            }
            _ => return Ok(false),
        };
        if !function.parameters.iter().any(|parameter| parameter.name == pointer_name) {
            return Ok(false);
        }
        let (class, incoming) = match self.locations.get(pointer_name) {
            Some(location) => (location.class, location.register),
            None => return Ok(false),
        };
        if class != ValueClass::General {
            return Ok(false);
        }
        // The call's result must match the store width: a general (int) pointee needs an
        // int-returning call (result in r3, stw/stb/sth); a float/double pointee needs a
        // matching float-returning call (result in f1, stfs/stfd). A mismatch (int call to a
        // float target, or vice versa) would need a conversion — defer.
        let float_store = matches!(pointee, Pointee::Float | Pointee::Double);
        let matched = match pointee {
            Pointee::Float => self.call_return_types.get(name) == Some(&Type::Float),
            Pointee::Double => self.call_return_types.get(name) == Some(&Type::Double),
            _ => !matches!(self.call_return_types.get(name), Some(Type::Float | Type::Double)),
        };
        if !matched {
            return Ok(false);
        }
        // The call must NOT pass the saved pointer as an argument (that keeps it in an
        // argument register across the call — a different shape).
        if arguments.iter().any(|argument| expression_reads_name(argument, pointer_name)) {
            return Ok(false);
        }
        let offset = i16::try_from(byte_offset)
            .map_err(|_| Diagnostic::error("store-through-saved-pointer offset out of range (roadmap)"))?;

        // Callee-saved frame: r31 holds the pointer across the call; the store-sink epilogue
        // reloads LR before r31.
        let frame_size: i16 = 16;
        self.non_leaf = true;
        self.frame_size = frame_size;
        // Phase D: the saved pointer's home is a virtual — call-crossing -> r31; the
        // epilogue reload (emit_epilogue_and_return reads callee_saved) renames too.
        let saved = self.fresh_virtual_general();
        self.callee_saved = vec![saved];
        self.epilogue_lr_first = true;
        // The interleaved save+move prologue, from the FRAME BUILDER.
        self.output.instructions.extend(mwcc_vreg::FramePlan::sized_for(vec![saved]).prologue_interleaved(&[incoming]));
        if let Some(location) = self.locations.get_mut(pointer_name) {
            location.register = saved;
        }
        // A float-returning call leaves its result in f1 (stfs/stfd); an int call in r3.
        let result = if float_store {
            self.emit_call(name, arguments, None, true)?;
            mwcc_target::Eabi::float_result().number
        } else {
            self.emit_call(name, arguments, None, false)?;
            mwcc_target::Eabi::general_result().number
        };
        self.output.instructions.push(displacement_store(pointee, result, saved, offset));
        // A non-void function materializes its constant return value in r3 after the store.
        if let Some(return_expression) = function.return_expression.as_ref() {
            self.evaluate_tail(return_expression, function.return_type, mwcc_target::Eabi::general_result().number)?;
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// A MEMORY-loaded local carried across calls in r31: `int t = gi; g(); return t;`
    /// loads the global into r31 in the prologue, runs the calls, and returns it —
    /// `stwu; mflr; stw r0; stw r31; lwz r31,gi; bl; lwz r0; mr r3,r31; lwz r31; mtlr;
    /// addi; blr` (the `mr` rides between the LR and r31 reloads). A computed-index
    /// global-array element (`int t = arr[i]; g(); return t;` — the signal.c handler
    /// fetch) interleaves the address build into the prologue: `stwu; mflr; lis r4;
    /// stw r0; slwi r0,i; addi r3,r4; stw r31; lwzx r31,r3,r0; bl; …`. Call arguments
    /// must be constants (a register argument after a call reads clobbered state).
    /// A guarded call through a GLOBAL function pointer held in a local (the signal.c
    /// dispatch tail): `F t = gf; if (!t) return; t();` loads the pointer into r12,
    /// tests it, branches to the shared epilogue when the guard fires, and calls
    /// through — `stwu; mflr; stw r0; lwz r12,gf; cmplwi r12,0; beq EPILOGUE; mtctr;
    /// bctrl; EPILOGUE: lwz r0; mtlr; addi; blr`. Zero-argument, void, single call.
    fn try_guarded_global_pointer_call(&mut self, function: &Function) -> Compilation<bool> {
        if function.return_type != Type::Void
            || !function.guards.is_empty()
            || function.locals.len() != 1
            || function.return_expression.is_some()
        {
            return Ok(false);
        }
        let local = &function.locals[0];
        if local.is_static {
            return Ok(false);
        }
        let Some(Expression::Variable(global)) = &local.initializer else {
            return Ok(false);
        };
        if !self.globals.contains_key(global.as_str()) || self.global_array_sizes.contains_key(global.as_str()) {
            return Ok(false);
        }
        let [Statement::If { condition, then_body, else_body }, Statement::Expression(Expression::Call { name, arguments })] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        if !matches!(then_body.as_slice(), [Statement::Return(None)]) || !else_body.is_empty() {
            return Ok(false);
        }
        if name != &local.name {
            return Ok(false);
        }
        // Arguments must ALREADY sit in their argument registers (`t(s)` with `s` the
        // first parameter): nothing to materialize, so the sequence is identical to the
        // zero-argument form. Anything needing placement defers.
        for (position, argument) in arguments.iter().enumerate() {
            let Expression::Variable(argument_name) = argument else {
                return Ok(false);
            };
            let expected = mwcc_target::Eabi::FIRST_GENERAL_ARGUMENT + position as u8;
            match self.locations.get(argument_name) {
                Some(location) if location.class == ValueClass::General && location.register == expected => {}
                _ => return Ok(false),
            }
        }

        // The canonical saveless non-leaf frame, derived by the FRAME BUILDER — the
        // first consumer of the plan-based prologue (its epilogue is the standard
        // emit_epilogue_and_return form, identical to plan.epilogue()).
        let plan = mwcc_vreg::FramePlan::sized_for(Vec::new());
        self.non_leaf = true;
        self.frame_size = plan.frame_size;
        self.output.instructions.extend(plan.prologue());
        self.emit_global_load_value(global, 12)?;
        // The pointer local is UNSIGNED (cmplwi) and lives in r12 for the test.
        self.locations.insert(
            local.name.clone(),
            Location { class: ValueClass::General, register: 12, signed: false, width: 32, pointee: None, stride: None },
        );
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        // The guard branches ON TRUE straight to the shared epilogue (the bare-void fold).
        // The branch label and the staged pointer load advance the anonymous-`@N` counter.
        self.output.anonymous_label_bump = 3;
        let epilogue_branch = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options: options ^ 8, condition_bit, target: 0 });
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        let epilogue_label = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[epilogue_branch] {
            *target = epilogue_label;
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    fn try_callee_saved_memory_local(&mut self, function: &Function) -> Compilation<bool> {
        if !function.guards.is_empty()
            || function.locals.len() != 1
            || !matches!(function.return_type, Type::Int | Type::UnsignedInt)
            || function.statements.is_empty()
        {
            return Ok(false);
        }
        let local = &function.locals[0];
        if local.is_static || (local.declared_type.width() as u32) < 32 {
            return Ok(false);
        }
        let Some(initializer) = &local.initializer else {
            return Ok(false);
        };
        // The return is the local itself, or a two-leaf expression over the local and
        // ONE parameter — the parameter then survives the calls in r30 alongside the
        // local's r31 (`int t = gi; g(); return t + s;` → `stw r30,8; mr r30,r3;
        // lwz r31; bl; add r3,r31,r30`).
        let paired_parameter: Option<&str> = match function.return_expression.as_ref() {
            Some(Expression::Variable(returned)) if returned == &local.name => None,
            Some(Expression::Binary { left, right, .. }) => {
                let (Expression::Variable(first), Expression::Variable(second)) = (left.as_ref(), right.as_ref()) else {
                    return Ok(false);
                };
                let other = if first == &local.name {
                    second
                } else if second == &local.name {
                    first
                } else {
                    return Ok(false);
                };
                if !function.parameters.iter().any(|parameter| &parameter.name == other) {
                    return Ok(false);
                }
                Some(other.as_str())
            }
            _ => return Ok(false),
        };
        // An optional LEADING guard CHAIN reading the loaded local: `int t = gi; if (!t)
        // return -1; if (t == 1) return 0; g(); return t;` — the raise() shape. Every
        // guard compares the STAGED r0 copy (still valid — no call intervenes), each
        // constant early return branches to the shared epilogue, and only the first
        // compare carries the `mr r31,r0` in its latency slot.
        let mut guard_chain: Vec<(&Expression, i16)> = Vec::new();
        let mut rest = function.statements.as_slice();
        while let Some((Statement::If { condition, then_body, else_body }, tail)) = rest.split_first() {
            if !else_body.is_empty() || !matches!(then_body.as_slice(), [Statement::Return(Some(_))]) {
                break;
            }
            let [Statement::Return(Some(value))] = then_body.as_slice() else {
                break;
            };
            let Some(constant) = constant_value(value).and_then(|constant| i16::try_from(constant).ok()) else {
                return Ok(false);
            };
            guard_chain.push((condition, constant));
            rest = tail;
        }
        let guard = (!guard_chain.is_empty()).then_some(());
        // An optional CONDITIONAL STORE back into the loaded element (`int t = garr[i];
        // if (t != 1) garr[i] = 0; g(); return t;` — the raise() handler-reset). The
        // scaled index survives in its own register for the store's reuse of the
        // address. Verified without return-guards; the mixed chain defers.
        let (conditional_store, calls) = match rest.split_first() {
            Some((Statement::If { condition, then_body, else_body }, tail))
                if guard_chain.is_empty()
                    && else_body.is_empty()
                    && matches!(then_body.as_slice(), [Statement::Store { .. }]) =>
            {
                let [Statement::Store { target, value }] = then_body.as_slice() else {
                    return Ok(false);
                };
                let Some(constant) = constant_value(value).and_then(|constant| i16::try_from(constant).ok()) else {
                    return Ok(false);
                };
                (Some((condition, target, constant)), tail)
            }
            _ => (None, rest),
        };
        if calls.is_empty() {
            return Ok(false);
        }
        for statement in calls {
            let Statement::Expression(Expression::Call { arguments, .. }) = statement else {
                return Ok(false);
            };
            // An ARGUMENT call reshapes the whole sequence: mwcc keeps the array base
            // out of r3 and hoists the argument materialization into the address-build
            // latency (`addi r4,r4; li r3,0; stw r31; lwzx r31,r4,r0`) — a scheduling
            // composition not yet modeled. Zero-argument calls only.
            if !arguments.is_empty() {
                return Ok(false);
            }
        }
        // The two captured load forms: a scalar global, or a plain-index element of a
        // word-sized global array.
        enum MemoryLoad<'e> {
            Scalar,
            Array { name: &'e str, index: &'e Expression },
        }
        let load = match initializer {
            Expression::Variable(name)
                if self.globals.contains_key(name.as_str()) && !self.global_array_sizes.contains_key(name.as_str()) =>
            {
                if pointee_of_type(self.globals[name.as_str()]) != Some(Pointee::Int)
                    && pointee_of_type(self.globals[name.as_str()]) != Some(Pointee::UnsignedInt)
                {
                    return Ok(false);
                }
                MemoryLoad::Scalar
            }
            Expression::Index { base, index } => {
                let Expression::Variable(name) = base.as_ref() else { return Ok(false) };
                if !self.global_array_sizes.contains_key(name.as_str()) || constant_value(index).is_some() {
                    return Ok(false);
                }
                if !matches!(index.as_ref(), Expression::Variable(_)) {
                    return Ok(false);
                }
                if !matches!(pointee_of_type(self.globals[name.as_str()]), Some(Pointee::Int | Pointee::UnsignedInt)) {
                    return Ok(false);
                }
                MemoryLoad::Array { name, index }
            }
            _ => return Ok(false),
        };

        // The PAIRED form is verified for the guard-free scalar load only.
        if paired_parameter.is_some() && (guard.is_some() || matches!(load, MemoryLoad::Array { .. })) {
            return Ok(false);
        }
        // A multi-guard chain over the ARRAY form is unverified — defer.
        if guard_chain.len() > 1 && matches!(load, MemoryLoad::Array { .. }) {
            return Ok(false);
        }
        // The conditional store: verified for the ARRAY load storing back into the SAME
        // element (same array, same index variable), with no paired parameter. The
        // scaled index survives in its own register and the base/index pair is reused:
        // `lis r4; slwi r5,i,2; stw r0; addi r3,r4; stw r31; lwzx r31,r3,r5;
        // cmpwi r31,K; beq SKIP; li r0,C; stwx r0,r3,r5; SKIP: bl; …`.
        if let Some((store_condition, store_target, store_constant)) = conditional_store {
            if paired_parameter.is_some() {
                return Ok(false);
            }
            let MemoryLoad::Array { name: load_name, index: load_index } = load else {
                return Ok(false);
            };
            let Expression::Index { base, index } = store_target else {
                return Ok(false);
            };
            let Expression::Variable(store_name) = base.as_ref() else {
                return Ok(false);
            };
            if store_name != load_name {
                return Ok(false);
            }
            let (Expression::Variable(load_index_name), Expression::Variable(store_index_name)) =
                (load_index, index.as_ref())
            else {
                return Ok(false);
            };
            if load_index_name != store_index_name {
                return Ok(false);
            }

            self.non_leaf = true;
            self.frame_size = 16;
            self.callee_saved = vec![31];
            self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
            self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
            let signed = !matches!(local.declared_type, Type::UnsignedInt);
            // The scaled index lands past the (reserved) base-high register so both
            // survive for the store: `lis r4; slwi r5,i,2; stw r0,20; addi r3,r4;
            // stw r31,12; lwzx r31,r3,r5`.
            let index_register = self.general_register_of_leaf(load_index)?;
            let high = self.fresh_virtual_general();
            let scaled = self.fresh_virtual_general();
            self.emit_address_high(high, load_name);
            self.output.instructions.push(Instruction::ShiftLeftImmediate { a: scaled, s: index_register, shift: 2 });
            self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
            self.record_relocation(RelocationKind::Addr16Lo, load_name);
            self.output.instructions.push(Instruction::AddImmediate { d: index_register, a: high, immediate: 0 });
            let saved = self.fresh_virtual_general();
            self.callee_saved = vec![saved];
            self.output.instructions.push(Instruction::StoreWord { s: saved, a: 1, offset: 12 });
            self.output.instructions.push(Instruction::LoadWordIndexed { d: saved, a: index_register, b: scaled });
            self.locations.insert(
                local.name.clone(),
                Location { class: ValueClass::General, register: saved, signed, width: 32, pointee: None, stride: None },
            );
            // The conditional store skips on the condition's FALSE side, the value
            // materializes into r0, and the base/scaled pair is reused. (@N: measured
            // against the real extab numbering.)
            self.output.anonymous_label_bump = 3;
            let (options, condition_bit) = self.emit_condition_test(store_condition)?;
            let skip_branch = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
            self.output.instructions.push(Instruction::AddImmediate { d: GENERAL_SCRATCH, a: 0, immediate: store_constant });
            self.output.instructions.push(Instruction::StoreWordIndexed { s: GENERAL_SCRATCH, a: index_register, b: scaled });
            let skip_label = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[skip_branch] {
                *target = skip_label;
            }
            for statement in calls {
                self.emit_statement(statement)?;
            }
            let result = mwcc_target::Eabi::general_result().number;
            self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
            self.output.instructions.push(Instruction::Or { a: result, s: saved, b: saved });
            self.output.instructions.push(Instruction::LoadWord { d: saved, a: 1, offset: 12 });
            self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
            self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            return Ok(true);
        }
        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved = if paired_parameter.is_some() { vec![31, 30] } else { vec![31] };
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        let signed = !matches!(local.declared_type, Type::UnsignedInt);
        // Phase D: the callee-saved home is a VIRTUAL in every form — its range
        // crosses the calls, so the allocator assigns it from the callee-saved pool
        // (r31), and apply() rewrites the saves, loads, moves, and restores together.
        // (The paired form allocates its second virtual below; creation order makes
        // the ids deterministic: the local first -> r31, the parameter -> r30.)
        let saved: u8 = self.fresh_virtual_general();
        // The paired parameter saves in r30 between the r31 save and the memory load:
        // `stw r31,12; stw r30,8; mr r30,<param>; lwz r31,<gi>`.
        if let Some(parameter) = paired_parameter {
            let Some(incoming) = self.lookup_general(parameter) else {
                return Ok(false);
            };
            // The parameter's callee-saved home is the SECOND virtual: created after
            // `saved`, both widen to entry, so the scan assigns saved->r31, pair->r30.
            let pair = self.fresh_virtual_general();
            self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
            self.output.instructions.push(Instruction::StoreWord { s: saved, a: 1, offset: 12 });
            self.output.instructions.push(Instruction::StoreWord { s: pair, a: 1, offset: 8 });
            self.output.instructions.push(Instruction::Or { a: pair, s: incoming, b: incoming });
            if let Some(location) = self.locations.get_mut(parameter) {
                location.register = pair;
            }
            self.evaluate_general(initializer, saved)?;
            self.locations.insert(
                local.name.clone(),
                Location { class: ValueClass::General, register: saved, signed, width: 32, pointee: None, stride: None },
            );
            for statement in calls {
                self.emit_statement(statement)?;
            }
            // The epilogue computes the return expression in the slot after the LR
            // reload: `lwz r0,20; add r3,r31,r30; lwz r31,12; lwz r30,8; mtlr; addi; blr`.
            let result = mwcc_target::Eabi::general_result().number;
            self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
            self.evaluate_tail(function.return_expression.as_ref().expect("checked above"), function.return_type, result)?;
            self.output.instructions.push(Instruction::LoadWord { d: saved, a: 1, offset: 12 });
            self.output.instructions.push(Instruction::LoadWord { d: pair, a: 1, offset: 8 });
            self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
            self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            return Ok(true);
        }
        match load {
            MemoryLoad::Scalar => {
                self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
                self.output.instructions.push(Instruction::StoreWord { s: saved, a: 1, offset: 12 });
                if guard.is_some() {
                    // With a guard the load STAGES through r0: the compare reads r0 and
                    // the `mr r31,r0` fills its latency slot — `lwz r0,gi; cmpwi r0,0;
                    // mr r31,r0; bne CONT` (the guard emission below issues the branch).
                    self.evaluate_general(initializer, GENERAL_SCRATCH)?;
                    self.locations.insert(
                        local.name.clone(),
                        Location { class: ValueClass::General, register: GENERAL_SCRATCH, signed, width: 32, pointee: None, stride: None },
                    );
                } else {
                    self.evaluate_general(initializer, saved)?;
                }
            }
            MemoryLoad::Array { name, index } => {
                let index_register = self.general_register_of_leaf(index)?;
                let high = self.fresh_virtual_general();
                self.emit_address_high(high, name);
                self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
                self.output.instructions.push(Instruction::ShiftLeftImmediate { a: GENERAL_SCRATCH, s: index_register, shift: 2 });
                self.record_relocation(RelocationKind::Addr16Lo, name);
                self.output.instructions.push(Instruction::AddImmediate { d: index_register, a: high, immediate: 0 });
                self.output.instructions.push(Instruction::StoreWord { s: saved, a: 1, offset: 12 });
                self.output.instructions.push(Instruction::LoadWordIndexed { d: saved, a: index_register, b: GENERAL_SCRATCH });
            }
        }
        let result = mwcc_target::Eabi::general_result().number;
        if guard.is_some() {
            // Each guard tests the just-loaded value (the staged r0 copy for a scalar —
            // valid across the whole chain, no call intervenes — or r31 for the array
            // form), then `li r3,K; b EPILOGUE`; the next guard or the calls are the
            // fall-through. Only the FIRST compare carries the `mr r31,r0` in its
            // latency slot. A multi-guard array chain is unverified — deferred above.
            // The labels advance mwcc's anonymous-`@N` counter: one per guard's
            // fall-through label plus the shared epilogue; the staged scalar load adds
            // one more (measured against the real extab/extabindex `@N` numbering).
            self.output.anonymous_label_bump =
                2 * guard_chain.len() as u32 + if matches!(load, MemoryLoad::Scalar) { 1 } else { 0 };
            if matches!(load, MemoryLoad::Array { .. }) {
                self.locations.insert(
                    local.name.clone(),
                    Location { class: ValueClass::General, register: saved, signed, width: 32, pointee: None, stride: None },
                );
            }
            let mut epilogue_branches = Vec::new();
            for (position, (condition, early_constant)) in guard_chain.iter().enumerate() {
                let (options, condition_bit) = self.emit_condition_test(condition)?;
                if position == 0 && matches!(load, MemoryLoad::Scalar) {
                    self.output.instructions.push(Instruction::Or { a: saved, s: GENERAL_SCRATCH, b: GENERAL_SCRATCH });
                }
                let skip_branch = self.output.instructions.len();
                self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
                self.output.instructions.push(Instruction::AddImmediate { d: result, a: 0, immediate: *early_constant });
                epilogue_branches.push(self.output.instructions.len());
                self.output.instructions.push(Instruction::Branch { target: 0 });
                let fall_through = self.output.instructions.len();
                if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[skip_branch] {
                    *target = fall_through;
                }
            }
            self.locations.insert(
                local.name.clone(),
                Location { class: ValueClass::General, register: saved, signed, width: 32, pointee: None, stride: None },
            );
            for statement in calls {
                self.emit_statement(statement)?;
            }
            self.output.instructions.push(Instruction::Or { a: result, s: saved, b: saved });
            let epilogue_label = self.output.instructions.len();
            for branch in epilogue_branches {
                if let Instruction::Branch { target } = &mut self.output.instructions[branch] {
                    *target = epilogue_label;
                }
            }
            // With the result already placed on both paths, the epilogue is plain:
            // `lwz r0,20; lwz r31,12; mtlr; addi; blr`.
            self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
            self.output.instructions.push(Instruction::LoadWord { d: saved, a: 1, offset: 12 });
            self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
            self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            return Ok(true);
        }
        self.locations.insert(
            local.name.clone(),
            Location { class: ValueClass::General, register: saved, signed, width: 32, pointee: None, stride: None },
        );
        for statement in calls {
            self.emit_statement(statement)?;
        }
        // The epilogue interleaves the result move between the LR and callee-saved
        // reloads. `saved` is the virtual for the guard-free scalar (apply() rewrites
        // the restore's field with the value's home) and the literal r31 otherwise.
        self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: 20 });
        self.output.instructions.push(Instruction::Or { a: result, s: saved, b: saved });
        self.output.instructions.push(Instruction::LoadWord { d: saved, a: 1, offset: 12 });
        self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: 16 });
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// Decode `*p = call()` / `p[const] = call()` / `p->m = call()` where the call is an
    /// integer-returning, zero-argument call and `p` is a General-class pointer variable —
    /// yielding (pointer name, byte offset, stored pointee, call name). Shared by the
    /// two-output-pointer store sink.
    fn decode_pointer_call_store(&self, statement: &Statement) -> Option<(String, i16, Pointee, String)> {
        let Statement::Store { target, value } = statement else { return None };
        let Expression::Call { name, arguments } = value else { return None };
        if !arguments.is_empty() {
            return None;
        }
        if matches!(self.call_return_types.get(name), Some(Type::Float | Type::Double)) {
            return None;
        }
        let (pointer_name, byte_offset, pointee): (&str, i64, Pointee) = match target {
            Expression::Dereference { pointer } => {
                let Expression::Variable(name) = pointer.as_ref() else { return None };
                (name, 0, self.pointee_of(pointer).ok()?)
            }
            Expression::Index { base, index } => {
                let Expression::Variable(name) = base.as_ref() else { return None };
                let constant = constant_value(index)?;
                let pointee = self.pointee_of(base).ok()?;
                (name, constant * pointee.size() as i64, pointee)
            }
            Expression::Member { base, offset, member_type, index_stride: None } => {
                let Expression::Variable(name) = base.as_ref() else { return None };
                let pointee = pointee_of_type(*member_type)?;
                (name, *offset as i64, pointee)
            }
            _ => return None,
        };
        if matches!(pointee, Pointee::Float | Pointee::Double) {
            return None;
        }
        let offset = i16::try_from(byte_offset).ok()?;
        Some((pointer_name.to_string(), offset, pointee, name.clone()))
    }

    /// Two to four output pointers, each receiving a call result: `void s(int*a,int*b){ *a=g();
    /// *b=h(); }`. Every pointer must survive its call, so mwcc parks them in callee-saved
    /// registers — the pointer arriving in the HIGHEST incoming register in r31, the next in r30,
    /// and so on descending (positional, independent of store order) — runs each call, stores its
    /// result, then reloads LR before all the saved GPRs. The single-pointer case is
    /// `try_store_call_through_pointer`.
    fn try_stores_through_pointers(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.locals.is_empty() {
            return Ok(false);
        }
        if function.return_type != Type::Void || function.return_expression.is_some() {
            return Ok(false);
        }
        let count = function.statements.len();
        if !(2..=4).contains(&count) {
            return Ok(false);
        }
        // Every statement is `*p = call()` through a distinct General-class pointer parameter.
        let mut decoded = Vec::with_capacity(count);
        for statement in &function.statements {
            match self.decode_pointer_call_store(statement) {
                Some(store) => decoded.push(store),
                None => return Ok(false),
            }
        }
        let mut incoming = Vec::with_capacity(count);
        for (pointer_name, _, _, _) in &decoded {
            if !function.parameters.iter().any(|parameter| &parameter.name == pointer_name) {
                return Ok(false);
            }
            match self.locations.get(pointer_name) {
                Some(location) if location.class == ValueClass::General => incoming.push(location.register),
                _ => return Ok(false),
            }
        }
        let mut distinct = incoming.clone();
        distinct.sort_unstable();
        distinct.dedup();
        if distinct.len() != count {
            return Ok(false);
        }

        // Assign r31, r30, … to the pointers by DESCENDING incoming register (highest -> r31).
        let mut order: Vec<usize> = (0..count).collect();
        order.sort_by(|&i, &j| incoming[j].cmp(&incoming[i]));
        // Phase D: each saved pointer's home is a virtual, created in DESCENDING
        // incoming order — all widen to entry via their saves, so the scan assigns
        // by id: first virtual -> r31, next -> r30, … exactly the positional rule.
        let mut saved_reg = vec![0u8; count];
        let mut callee_saved = Vec::with_capacity(count);
        for &index in order.iter() {
            let register = self.fresh_virtual_general();
            saved_reg[index] = register;
            callee_saved.push(register);
        }

        // The interleaved save+move prologue, from the FRAME BUILDER (each pointer
        // parks in its callee-saved home right after that home's save).
        let plan = mwcc_vreg::FramePlan::sized_for(callee_saved.clone());
        self.non_leaf = true;
        self.frame_size = plan.frame_size;
        self.callee_saved = callee_saved;
        self.epilogue_lr_before_gprs = true;
        let incoming_ordered: Vec<u8> = order.iter().map(|&index| incoming[index]).collect();
        self.output.instructions.extend(plan.prologue_interleaved(&incoming_ordered));
        for (index, (pointer_name, _, _, _)) in decoded.iter().enumerate() {
            if let Some(location) = self.locations.get_mut(pointer_name) {
                location.register = saved_reg[index];
            }
        }

        // Each call in source order, its result stored through the saved pointer.
        let result = mwcc_target::Eabi::general_result().number;
        for (index, (_, offset, pointee, call)) in decoded.iter().enumerate() {
            self.emit_call(call, &[], None, false)?;
            self.output.instructions.push(displacement_store(*pointee, result, saved_reg[index], *offset));
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// A void function whose body is two or more calls that each pass the SAME argument
    /// list — all the parameters, in order — `f(a,b){ g(a,b); h(a,b); }` (the single-
    /// parameter `f(x){ g(x); h(x); }` is the common case). Each parameter is live across
    /// the calls, so mwcc saves them in callee-saved registers up front (r31 to the last
    /// parameter, descending), interleaving each save with its move; the first call uses
    /// the incoming argument registers directly (no moves), and each later call restores
    /// them. One of the most common real shapes (a state handed to several functions).
    fn try_callee_saved_call_args(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.locals.is_empty() {
            return Ok(false);
        }
        if function.parameters.is_empty() || matches!(function.return_type, Type::Float | Type::Double) {
            return Ok(false);
        }
        // A non-void function returns one of the parameters (or a register-only expression
        // over them) after the calls — the return is the post-call use that keeps them
        // live; a void function needs two or more calls for that liveness. A call in the
        // return is a different shape (call-result), so it defers.
        let returns_value = function.return_type != Type::Void;
        if returns_value {
            match &function.return_expression {
                Some(expression) if !expression_has_call(expression) => {}
                _ => return Ok(false),
            }
        } else if function.statements.len() < 2 {
            return Ok(false);
        }
        // Every statement must be a call whose arguments are exactly the parameters in
        // order, so the first call needs no moves and the live set is all the parameters.
        for statement in &function.statements {
            let Statement::Expression(Expression::Call { arguments, .. }) = statement else { return Ok(false) };
            if arguments.len() != function.parameters.len() {
                return Ok(false);
            }
            for (argument, parameter) in arguments.iter().zip(&function.parameters) {
                if !matches!(argument, Expression::Variable(name) if name == &parameter.name) {
                    return Ok(false);
                }
            }
        }
        // Each parameter's incoming register; all must be general-class.
        let mut incoming = Vec::new();
        for parameter in &function.parameters {
            match self.locations.get(&parameter.name) {
                Some(location) if location.class == ValueClass::General => incoming.push((parameter.name.clone(), location.register)),
                _ => return Ok(false),
            }
        }
        let count = incoming.len();
        let frame_size = ((8 + 4 * count as i32 + 15) / 16 * 16) as i16;
        self.non_leaf = true;
        self.frame_size = frame_size;
        // Phase D: virtual homes, highest-rank first; the interleaved save+move
        // prologue comes from the FRAME BUILDER.
        let homes: Vec<u8> = (0..count).map(|_| self.fresh_virtual_general()).collect();
        self.callee_saved = homes.clone();
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        debug_assert_eq!(plan.frame_size, frame_size);
        let incoming_ordered: Vec<u8> = incoming.iter().rev().map(|(_, register)| *register).collect();
        self.output.instructions.extend(plan.prologue_interleaved(&incoming_ordered));
        // The first call finds the parameters still in their incoming registers (no
        // moves); afterward they live only in their callee-saved registers.
        self.emit_statement(&function.statements[0])?;
        for (rank, (name, _)) in incoming.iter().rev().enumerate() {
            let register = homes[rank];
            if let Some(location) = self.locations.get_mut(name) {
                location.register = register;
            }
        }
        for statement in &function.statements[1..] {
            self.emit_statement(statement)?;
        }
        // A non-void return reads the parameters from their callee-saved registers; the
        // epilogue scheduler hoists the LR reload ahead of this move, matching mwcc.
        if returns_value {
            let result = Eabi::general_result().number;
            let return_expression = function.return_expression.as_ref().unwrap();
            self.evaluate_tail(return_expression, function.return_type, result)?;
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// `return f(...) + x;` — a single general parameter `x` kept live across a call that sits INSIDE
    /// the return expression, then combined with the call's result. mwcc saves `x` in r31 before the
    /// call (`mr r31,r3`), runs the call (whose argument, when it is `x`, is already in the incoming
    /// register, so no move precedes it), reloads LR, then combines from the callee-saved register
    /// (`add r3,r31,r3` — the saved value first). The call is argument-free or forwards exactly the
    /// parameter; a computed/constant argument schedules its materialization differently and defers.
    /// Handles the low-latency ops `+ | & ^` (commutative — `OP r3,r31,r3` on either source side) and
    /// `-` (its `subf` operands chosen by the call's side); heavier ops (e.g. `*`) and multi-parameter
    /// shapes are follow-ups.
    fn try_callee_saved_call_combine(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.locals.is_empty() || !function.statements.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        if function.parameters.len() != 1 {
            return Ok(false);
        }
        let param = &function.parameters[0];
        let (class, param_register) = match self.locations.get(&param.name) {
            Some(location) => (location.class, location.register),
            None => return Ok(false),
        };
        if class != ValueClass::General {
            return Ok(false);
        }
        let Some(Expression::Binary { operator, left, right }) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        // Low-latency ops mwcc issues as a single register op combining the saved parameter (r31)
        // and the call result (r3). The commutative ops (`+ | & ^`) use `OP r3,r31,r3` on either
        // source side; the non-commutative `-` picks its `subf` operands by which side the call is on.
        // `*` combines to a single `mullw r3,r31,r3`; mwcc issues it BEFORE the LR reload (overlapping
        // the multiply latency with the load), which the LR-reload hoist now models.
        if !matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract | BinaryOperator::Multiply | BinaryOperator::BitOr | BinaryOperator::BitAnd | BinaryOperator::BitXor) {
            return Ok(false);
        }
        let is_param = |expression: &Expression| matches!(expression, Expression::Variable(name) if name == &param.name);
        let (call_name, call_arguments, call_on_left) = match (left.as_ref(), right.as_ref()) {
            (Expression::Call { name, arguments }, other) if is_param(other) => (name, arguments, true),
            (other, Expression::Call { name, arguments }) if is_param(other) => (name, arguments, false),
            _ => return Ok(false),
        };
        // The call takes no arguments or forwards exactly the parameter (already in its incoming
        // register); anything else materializes an argument on a different schedule.
        if !(call_arguments.is_empty() || (call_arguments.len() == 1 && is_param(&call_arguments[0]))) {
            return Ok(false);
        }
        // Prologue: a 16-byte frame saving the link register and the saved parameter.
        self.non_leaf = true;
        self.frame_size = 16;
        // Phase D: the saved parameter's home is a virtual (call-crossing -> r31).
        let saved = self.fresh_virtual_general();
        self.callee_saved = vec![saved];
        // The canonical single-save frame, from the FRAME BUILDER.
        self.output.instructions.extend(mwcc_vreg::FramePlan::sized_for(vec![saved]).prologue());
        // Save the live parameter before the call clobbers its incoming register.
        self.output.instructions.push(Instruction::Or { a: saved, s: param_register, b: param_register });
        self.emit_call(call_name, call_arguments, None, false)?;
        // Combine the saved parameter with the call result (r3) — the saved value first.
        let result = Eabi::general_result().number;
        self.output.instructions.push(match operator {
            BinaryOperator::Add => Instruction::Add { d: result, a: saved, b: result },
            BinaryOperator::BitOr => Instruction::Or { a: result, s: saved, b: result },
            BinaryOperator::BitAnd => Instruction::And { a: result, s: saved, b: result },
            BinaryOperator::BitXor => Instruction::Xor { a: result, s: saved, b: result },
            // `subf d,a,b` computes `b - a`. `f()-x` (call left) is result-param -> `subf r3,r31,r3`;
            // `x-f()` (call right) is param-result -> `subf r3,r3,r31`.
            BinaryOperator::Subtract if call_on_left => Instruction::SubtractFrom { d: result, a: saved, b: result },
            BinaryOperator::Subtract => Instruction::SubtractFrom { d: result, a: result, b: saved },
            BinaryOperator::Multiply => Instruction::MultiplyLow { d: result, a: saved, b: result },
            _ => unreachable!("operator restricted above"),
        });
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// `p(x); q(y);` — two single-argument calls passing two distinct parameters in order (a `void`
    /// body). The second parameter is live across the first call, so mwcc saves it in r31 up front
    /// (`mr r31,r4`), runs the first call (the first parameter is still in its incoming register), then
    /// moves the saved second parameter into place for the second call (`mr r3,r31; bl`). The epilogue
    /// reloads LR (hoisted right after the last call) then restores r31. Exactly two parameters/two
    /// calls for now — longer sequences assign further callee-saved registers and are a follow-up.
    fn try_callee_saved_call_sequence(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.locals.is_empty() {
            return Ok(false);
        }
        if function.return_type != Type::Void || function.return_expression.is_some() {
            return Ok(false);
        }
        if function.parameters.len() != 2 {
            return Ok(false);
        }
        let [Statement::Expression(Expression::Call { name: name0, arguments: args0 }), Statement::Expression(Expression::Call { name: name1, arguments: args1 })] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        let is_param = |expression: &Expression, index: usize| matches!(expression, Expression::Variable(name) if name == &function.parameters[index].name);
        if args0.len() != 1 || !is_param(&args0[0], 0) || args1.len() != 1 || !is_param(&args1[0], 1) {
            return Ok(false);
        }
        // Both parameters must be general-class (a float parameter is passed/saved differently).
        let mut param_registers = Vec::new();
        for parameter in &function.parameters {
            match self.locations.get(&parameter.name) {
                Some(location) if location.class == ValueClass::General => param_registers.push(location.register),
                _ => return Ok(false),
            }
        }
        // Prologue: a 16-byte frame saving the link register and the saved parameter.
        self.non_leaf = true;
        self.frame_size = 16;
        // Phase D: the saved parameter's home is a virtual (call-crossing -> r31).
        let saved = self.fresh_virtual_general();
        self.callee_saved = vec![saved];
        // The canonical single-save frame, from the FRAME BUILDER.
        self.output.instructions.extend(mwcc_vreg::FramePlan::sized_for(vec![saved]).prologue());
        // Save the second parameter (live across the first call), and record it there so
        // the second call materializes its argument from the saved home (`mr r3,r31`).
        self.output.instructions.push(Instruction::Or { a: saved, s: param_registers[1], b: param_registers[1] });
        if let Some(location) = self.locations.get_mut(&function.parameters[1].name) {
            location.register = saved;
        }
        // First call: the first parameter is still in its incoming register (no move).
        self.emit_call(name0, args0, None, false)?;
        // Second call: the second parameter now lives in r31.
        self.emit_call(name1, args1, None, false)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// `return f() - g();` — two argument-free calls whose results are subtracted in the return. mwcc
    /// runs the first call, saves its result in r31 (`mr r31,r3`, live across the second call), runs
    /// the second call (its result in r3), reloads LR, then `subf r3,r3,r31` (= r31 - r3 = f() - g()).
    /// Only `-` for now: a COMMUTATIVE op evaluates its operands right-first in mwcc, reordering the
    /// symbol table, which the left-first `symbol_order` does not reproduce (a `referenced_names`
    /// change — deferred). Argument-bearing calls are a follow-up.
    fn try_callee_saved_two_call_combine(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.locals.is_empty() || !function.statements.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        // Only `-` for now. A commutative `+`/`|`/… evaluates its operands RIGHT-first in mwcc (so the
        // symbol/relocation order is right-then-left), which our left-first `symbol_order` does not
        // reproduce — that needs a `referenced_names` change and defers. `-` is natural left-then-right.
        let Some(Expression::Binary { operator: BinaryOperator::Subtract, left, right }) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        // Both operands are argument-free calls (an argument would interleave its materialization with
        // the saves on a schedule not modeled here).
        let (Expression::Call { name: first_name, arguments: first_arguments }, Expression::Call { name: second_name, arguments: second_arguments }) = (left.as_ref(), right.as_ref()) else {
            return Ok(false);
        };
        if !first_arguments.is_empty() || !second_arguments.is_empty() {
            return Ok(false);
        }
        // Prologue: a 16-byte frame saving the link register and the saved result.
        self.non_leaf = true;
        self.frame_size = 16;
        // Phase D: the first result's home is a virtual (call-crossing -> r31).
        let saved = self.fresh_virtual_general();
        self.callee_saved = vec![saved];
        // The canonical single-save frame, from the FRAME BUILDER.
        self.output.instructions.extend(mwcc_vreg::FramePlan::sized_for(vec![saved]).prologue());
        // First call; its result is saved across the second call.
        self.emit_call(first_name, first_arguments, None, false)?;
        self.output.instructions.push(Instruction::Or { a: saved, s: 3, b: 3 });
        // Second call; its result lands in r3.
        self.emit_call(second_name, second_arguments, None, false)?;
        // `subf d,a,b` = `b - a`; first result saved, second in r3, so `subf r3,r3,<saved>` =
        // first - second (`f() - g()`).
        self.output.instructions.push(Instruction::SubtractFrom { d: 3, a: 3, b: saved });
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// `g(x); return x OP y;` — TWO parameters both live across a single call (the first is passed to
    /// it, the second is only used in the return), combined by a low-latency op (`+ | & ^`, or `-`
    /// whose `subf` order `evaluate_tail` reproduces). mwcc
    /// preserves BOTH in callee-saved registers — the last parameter in r31, the first in r30 —
    /// saving them interleaved up front (`stw r31; mr r31,y; stw r30; mr r30,x`); the return combines
    /// from the saved registers (`add r3,r30,r31`). The call may pass EITHER parameter: the first stays
    /// in its incoming register (no move); the second is materialized from its saved r31 (`mr r3,r31`).
    fn try_callee_saved_param_pair_combine(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() || !function.locals.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        if function.parameters.len() != 2 {
            return Ok(false);
        }
        let [Statement::Expression(Expression::Call { name, arguments })] = function.statements.as_slice() else {
            return Ok(false);
        };
        // The call passes exactly one of the two parameters (the first stays in its incoming register;
        // the second is materialized from its callee-saved register — see the save/location logic).
        if arguments.len() != 1 || !matches!(&arguments[0], Expression::Variable(argument) if argument == &function.parameters[0].name || argument == &function.parameters[1].name) {
            return Ok(false);
        }
        // The return is `p OP q` reading both parameters, combined by a low-latency op whose operand
        // order `evaluate_tail` reproduces (source order for the commutative ops; the correct `subf`
        // for `-`). `*` is excluded here: with TWO saved GPRs mwcc interleaves the LR reload between
        // the register restores (`mullw; lwz r31; lwz r0; lwz r30`), a register-death epilogue schedule
        // this path does not model — the single-saved-GPR combine handles multiply.
        let Some(Expression::Binary { operator, left, right }) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        if !matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract | BinaryOperator::BitOr | BinaryOperator::BitAnd | BinaryOperator::BitXor) {
            return Ok(false);
        }
        let is_param = |expression: &Expression, index: usize| matches!(expression, Expression::Variable(name) if name == &function.parameters[index].name);
        if !((is_param(left, 0) && is_param(right, 1)) || (is_param(left, 1) && is_param(right, 0))) {
            return Ok(false);
        }
        // Both parameters general-class; keep them in incoming (parameter) order for the save loop.
        let mut incoming = Vec::new();
        for parameter in &function.parameters {
            match self.locations.get(&parameter.name) {
                Some(location) if location.class == ValueClass::General => incoming.push((parameter.name.clone(), location.register)),
                _ => return Ok(false),
            }
        }
        // Prologue: a 16-byte frame saving the link register and r31 + r30.
        let frame_size = 16i16;
        self.non_leaf = true;
        self.frame_size = frame_size;
        // Phase D: virtual homes, highest-rank first (id order -> r31, r30); the
        // interleaved save+move prologue comes from the FRAME BUILDER.
        let homes: Vec<u8> = (0..incoming.len()).map(|_| self.fresh_virtual_general()).collect();
        self.callee_saved = homes.clone();
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        debug_assert_eq!(plan.frame_size, frame_size);
        let incoming_ordered: Vec<u8> = incoming.iter().rev().map(|(_, register)| *register).collect();
        self.output.instructions.extend(plan.prologue_interleaved(&incoming_ordered));
        // The second parameter is now read from its callee-saved home (its incoming register
        // is dead), so a call passing it materializes `mr r3,r31`. The first parameter stays in its
        // incoming register for the call (no move) and moves to its home only afterward.
        if let Some(location) = self.locations.get_mut(&function.parameters[1].name) {
            location.register = homes[0];
        }
        self.emit_call(name, arguments, None, false)?;
        if let Some(location) = self.locations.get_mut(&function.parameters[0].name) {
            location.register = homes[1];
        }
        let result = Eabi::general_result().number;
        self.evaluate_tail(function.return_expression.as_ref().unwrap(), function.return_type, result)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// One or two locals that are CALL RESULTS, live across later calls, then returned:
    /// `int z = g(); h(); return z;` or `int a = g1(); int b = g2(); h(); return a+b;`.
    /// mwcc preserves them in r31 (and r30) across the later calls — each producing call
    /// is followed by a move into its callee-saved register, all saved up front. The
    /// single-local return may post-process z (`z + 1`); the two-local return must be a
    /// single low-latency op of both (`a + b`), as in [`Self::try_callee_saved`].
    /// (Parameters live across calls go through that path.) Narrowly shaped.
    fn try_callee_saved_call_result(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        // One or two general-int locals, each initialized by an argument-free call.
        let count = function.locals.len();
        if count == 0 || count > 2 {
            return Ok(false);
        }
        let mut init_calls: Vec<(String, Vec<Expression>)> = Vec::new();
        for local in &function.locals {
            if !matches!(local.declared_type, Type::Int | Type::UnsignedInt) {
                return Ok(false);
            }
            let Some(Expression::Call { name, arguments }) = local.initializer.as_ref() else {
                return Ok(false);
            };
            init_calls.push((name.clone(), arguments.clone()));
        }
        // A producing call's arguments are allowed only in the single-local case, and
        // only when they forward parameters in their natural register positions (arg i
        // is parameter i, all parameters general) — then the parameters are already in
        // place, no moves are emitted, and the sequence matches the argument-free shape.
        // A constant/reordered argument would schedule its materialization differently,
        // and a second producing call's parameter would be call-clobbered; both defer.
        let params_all_general = !function
            .parameters
            .iter()
            .any(|parameter| matches!(parameter.parameter_type, Type::Float | Type::Double));
        for (index, (_, arguments)) in init_calls.iter().enumerate() {
            if arguments.is_empty() {
                continue;
            }
            let forwards_parameters = count == 1
                && index == 0
                && params_all_general
                && arguments.len() <= function.parameters.len()
                && arguments
                    .iter()
                    .enumerate()
                    .all(|(position, argument)| matches!(argument, Expression::Variable(name) if name == &function.parameters[position].name));
            if !forwards_parameters {
                return Ok(false);
            }
        }
        // The return reads no parameter (it would be a call-clobbered register) and no
        // global (its load reschedules against the epilogue). A single local may be
        // post-processed (`z + 1`); two locals must combine in one low-latency op
        // (`a + b`), the only shape whose restores aren't rescheduled.
        let Some(return_expr) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        if function.parameters.iter().any(|parameter| expression_reads_name(return_expr, &parameter.name)) {
            return Ok(false);
        }
        if self.globals.keys().any(|name| expression_reads_name(return_expr, name)) {
            return Ok(false);
        }
        if count == 1 {
            if !expression_reads_name(return_expr, &function.locals[0].name) {
                return Ok(false);
            }
        } else {
            let single_op = matches!(return_expr, Expression::Binary { operator, left, right }
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract
                    | BinaryOperator::BitAnd | BinaryOperator::BitOr | BinaryOperator::BitXor)
                    && matches!(left.as_ref(), Expression::Variable(_))
                    && matches!(right.as_ref(), Expression::Variable(_)));
            if !single_op || !function.locals.iter().all(|local| expression_reads_name(return_expr, &local.name)) {
                return Ok(false);
            }
        }
        // The body is one or more straight-line argument-free calls (so the locals are
        // genuinely live across a call).
        if function.statements.is_empty() {
            return Ok(false);
        }
        for statement in &function.statements {
            let Statement::Expression(Expression::Call { arguments, .. }) = statement else {
                return Ok(false);
            };
            if !arguments.is_empty() {
                return Ok(false);
            }
        }

        // Prologue: a frame saving the link register and the callee-saved registers
        // (r31, then r30), all up front, highest at the top of the frame.
        let frame_size = (((8 + 4 * count as i32) + 15) / 16 * 16) as i16;
        self.non_leaf = true;
        self.frame_size = frame_size;
        // Phase D: virtual homes, created highest-rank first (id order -> r31, r30, …),
        // framed by the FRAME BUILDER (all saves consecutive — the canonical schedule).
        let homes: Vec<u8> = (0..count).map(|_| self.fresh_virtual_general()).collect();
        self.callee_saved = homes.clone();
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        debug_assert_eq!(plan.frame_size, frame_size);
        self.output.instructions.extend(plan.prologue());

        // Each local: its producing call, then move r3 into the local's callee-saved
        // register — the first local takes the lowest (r30 when there are two), the last
        // takes r31, matching mwcc's `bl g1; mr r30,r3; bl g2; mr r31,r3`.
        for (index, local) in function.locals.iter().enumerate() {
            let (init_name, init_arguments) = &init_calls[index];
            self.emit_call(init_name, init_arguments, None, false)?;
            // The first local takes the LOWEST home (homes are highest-first).
            let register = homes[count - 1 - index];
            self.output.instructions.push(Instruction::Or { a: register, s: 3, b: 3 });
            let signed = !matches!(local.declared_type, Type::UnsignedInt);
            self.locations.insert(
                local.name.clone(),
                Location { class: ValueClass::General, register, signed, width: 32, pointee: None, stride: None },
            );
        }

        // The later calls, then the return. The LR-reload hoist places the saved-LR
        // reload right after the last call, matching mwcc's epilogue order.
        for statement in &function.statements {
            self.emit_statement(statement)?;
        }
        let result = Eabi::general_result().number;
        self.evaluate_tail(return_expr, function.return_type, result)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// A single local COMPUTED from parameters (no call in its initializer) that is live
    /// across a call — passed to it and/or post-processed in the return:
    /// `int z = x + 1; g(z); return z;`. z is computed into r31 before the call, used
    /// from r31 (as a call argument and/or the return), then reloaded. Argument calls may
    /// pass only z and constants (a parameter would be call-clobbered).
    fn try_callee_saved_computed_local(&mut self, function: &Function) -> Compilation<bool> {
        if !self.frame_slots.is_empty() || !function.guards.is_empty() {
            return Ok(false);
        }
        if !matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        if function.locals.len() != 1 {
            return Ok(false);
        }
        let local = &function.locals[0];
        if !matches!(local.declared_type, Type::Int | Type::UnsignedInt) {
            return Ok(false);
        }
        let Some(initializer) = local.initializer.as_ref() else {
            return Ok(false);
        };
        // A genuinely computed initializer: not a bare variable (that keeps its source
        // register), not a call (the call-result path), reading no global.
        if matches!(initializer, Expression::Variable(_)) || expression_has_call(initializer) {
            return Ok(false);
        }
        if self.globals.keys().any(|name| expression_reads_name(initializer, name)) {
            return Ok(false);
        }
        // One or more argument calls whose arguments read only z (preserved in r31) and
        // constants; the return reads z and no parameter/global. (A parameter in either
        // would be read from a call-clobbered register.)
        if function.statements.is_empty() {
            return Ok(false);
        }
        let reads_param_or_global = |this: &Self, expression: &Expression| {
            function.parameters.iter().any(|parameter| expression_reads_name(expression, &parameter.name))
                || this.globals.keys().any(|name| expression_reads_name(expression, name))
        };
        for statement in &function.statements {
            let Statement::Expression(Expression::Call { arguments, .. }) = statement else {
                return Ok(false);
            };
            if arguments.iter().any(|argument| reads_param_or_global(self, argument)) {
                return Ok(false);
            }
        }
        let Some(return_expr) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        if !expression_reads_name(return_expr, &local.name) || reads_param_or_global(self, return_expr) {
            return Ok(false);
        }

        // Prologue, then compute z into r31, then the argument calls, then the return.
        self.non_leaf = true;
        self.frame_size = 16;
        // Phase D: the computed local's home is a virtual (call-crossing -> r31).
        let saved = self.fresh_virtual_general();
        self.callee_saved = vec![saved];
        // The canonical single-save frame, from the FRAME BUILDER.
        self.output.instructions.extend(mwcc_vreg::FramePlan::sized_for(vec![saved]).prologue());
        self.evaluate_general(initializer, saved)?;
        let signed = !matches!(local.declared_type, Type::UnsignedInt);
        self.locations.insert(
            local.name.clone(),
            Location { class: ValueClass::General, register: saved, signed, width: 32, pointee: None, stride: None },
        );
        for statement in &function.statements {
            self.emit_statement(statement)?;
        }
        let result = Eabi::general_result().number;
        self.evaluate_tail(return_expr, function.return_type, result)?;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// The raise FAMILY (the call-class acceptance target): a function-pointer
    /// local loaded from a static dispatch table, tested through guard blocks,
    /// conditionally cleared, and finally CALLED — with the local and the int
    /// parameter living in callee-saved registers across the calls. Every order
    /// below is the measured 44-instruction signal.c raise() capture; the
    /// registers are allocator-chosen (v_temp -> r31, v_sig -> r30 from the
    /// call-crossing pool; the address chain's virtual takes the freed r3).
    pub(crate) fn try_raise_family(&mut self, function: &Function) -> Compilation<bool> {
        macro_rules! decline {
            ($n:expr) => {{
                if std::env::var("RAISE_DEBUG").is_ok() {
                    eprintln!("raise decline {}", $n);
                }
                return Ok(false);
            }};
        }
        if !function.guards.is_empty() || function.return_type != Type::Int {
            decline!(1);
        }
        let [param] = function.parameters.as_slice() else { decline!(2) };
        if param.parameter_type != Type::Int {
            decline!(3);
        }
        let sig = param.name.as_str();
        let [local] = function.locals.as_slice() else { decline!(4) };
        if local.initializer.is_some() || local.array_length.is_some() {
            decline!(5);
        }
        let temp = local.name.as_str();
        if !matches!(&function.return_expression, Some(expression) if constant_value(expression) == Some(0)) {
            decline!(6);
        }
        let [s0, s1, s2, s3, s4, s5] = function.statements.as_slice() else { decline!(7) };
        let is_sig = |expression: &Expression| matches!(expression, Expression::Variable(name) if name == sig);
        let is_temp = |expression: &Expression| matches!(expression, Expression::Variable(name) if name == temp);
        // temp compared to a constant, through an optional cast (the source
        // writes `(unsigned long) temp != 1`).
        let temp_versus = |expression: &Expression, operator: BinaryOperator, constant: i64| -> bool {
            let Expression::Binary { operator: found, left, right } = expression else { return false };
            if *found != operator || constant_value(right) != Some(constant) {
                return false;
            }
            match left.as_ref() {
                Expression::Cast { operand, .. } => is_temp(operand),
                other => is_temp(other),
            }
        };
        // The table subscript `funcs[sig - 1]`, returning the table's name.
        let table_of = |expression: &Expression| -> Option<String> {
            let Expression::Index { base, index } = expression else { return None };
            let Expression::Variable(table) = base.as_ref() else { return None };
            let Expression::Binary { operator: BinaryOperator::Subtract, left, right } = index.as_ref() else { return None };
            (is_sig(left) && constant_value(right) == Some(1)).then(|| table.clone())
        };
        // s0: if (sig < 1 || sig > BOUND) return -1;
        let Statement::If { condition, then_body, else_body } = s0 else { decline!(8) };
        if !else_body.is_empty() || !matches!(then_body.as_slice(), [Statement::Return(Some(value))] if constant_value(value) == Some(-1)) {
            decline!(9);
        }
        let Expression::Binary { operator: BinaryOperator::LogicalOr, left, right } = condition else { decline!(10) };
        let low_test = matches!(left.as_ref(), Expression::Binary { operator: BinaryOperator::Less, left, right }
            if is_sig(left) && constant_value(right) == Some(1));
        let Expression::Binary { operator: BinaryOperator::Greater, left: bound_left, right: bound_right } = right.as_ref() else {
            decline!(11)
        };
        let Some(bound) = constant_value(bound_right).and_then(|bound| i16::try_from(bound).ok()) else { decline!(12) };
        if !low_test || !is_sig(bound_left) {
            decline!(13);
        }
        // s1: temp = funcs[sig - 1];
        let Statement::Assign { name: s1_name, value: s1_value } = s1 else { decline!(14) };
        let Some(table) = table_of(s1_value) else { decline!(15) };
        if s1_name != temp {
            decline!(16);
        }
        // s2: if ((cast) temp != 1) funcs[sig - 1] = 0;
        let Statement::If { condition, then_body, else_body } = s2 else { decline!(17) };
        if !else_body.is_empty() || !temp_versus(condition, BinaryOperator::NotEqual, 1) {
            decline!(18);
        }
        let [Statement::Store { target, value }] = then_body.as_slice() else { decline!(19) };
        if table_of(target).as_deref() != Some(table.as_str()) || !matches!(constant_value(value), Some(0)) {
            decline!(20);
        }
        // s3: if ((cast) temp == 1 || (temp == 0 && sig == 1)) return 0;
        let Statement::If { condition, then_body, else_body } = s3 else { decline!(21) };
        if !else_body.is_empty() || !matches!(then_body.as_slice(), [Statement::Return(Some(value))] if constant_value(value) == Some(0)) {
            decline!(22);
        }
        let Expression::Binary { operator: BinaryOperator::LogicalOr, left, right } = condition else { decline!(23) };
        if !temp_versus(left, BinaryOperator::Equal, 1) {
            decline!(24);
        }
        let Expression::Binary { operator: BinaryOperator::LogicalAnd, left: and_left, right: and_right } = right.as_ref() else {
            decline!(25)
        };
        if !temp_versus(and_left, BinaryOperator::Equal, 0)
            || !matches!(and_right.as_ref(), Expression::Binary { operator: BinaryOperator::Equal, left, right }
                if is_sig(left) && constant_value(right) == Some(1))
        {
            decline!(26);
        }
        // s4: if (temp == 0) exit(0);
        let Statement::If { condition, then_body, else_body } = s4 else { decline!(27) };
        if !else_body.is_empty() || !temp_versus(condition, BinaryOperator::Equal, 0) {
            decline!(28);
        }
        let [Statement::Expression(Expression::Call { name: exit_name, arguments })] = then_body.as_slice() else { decline!(29) };
        if arguments.len() != 1 || constant_value(&arguments[0]) != Some(0) {
            decline!(30);
        }
        let exit_name = exit_name.clone();
        // s5: temp(sig);
        if !matches!(s5, Statement::Expression(Expression::Call { name, arguments })
            if name == temp && arguments.len() == 1 && is_sig(&arguments[0]))
        {
            decline!(31);
        }


        // ---- emission (the measured 44-instruction schedule) ----
        self.frame_size = 16;
        self.non_leaf = true;
        self.epilogue_lr_before_gprs = true;
        let virtual_temp = self.fresh_virtual_general();
        let virtual_sig = self.fresh_virtual_general();
        self.callee_saved = vec![virtual_temp, virtual_sig];
        let plan = mwcc_vreg::FramePlan::sized_for(vec![virtual_temp, virtual_sig]);
        self.output.instructions.extend(plan.prologue());
        let result = Eabi::general_result().number;
        self.output.instructions.push(Instruction::move_register(virtual_sig, result));
        let taken = self.fresh_label();
        let load = self.fresh_label();
        let skip_store = self.fresh_label();
        let return_zero = self.fresh_label();
        let after = self.fresh_label();
        let call_label = self.fresh_label();
        let epilogue = self.fresh_label();
        // RANGE: blt into the taken block, ble past it to the load.
        self.output.instructions.push(Instruction::CompareWordImmediate { a: virtual_sig, immediate: 1 });
        self.emit_branch_conditional_to(12, 0, taken);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: virtual_sig, immediate: bound });
        self.emit_branch_conditional_to(4, 1, load);
        self.bind_label(taken);
        self.output.instructions.push(Instruction::load_immediate(result, -1));
        self.emit_branch_to(epilogue);
        // LOAD: the address chain in a fresh virtual (takes the freed r3), the
        // element folded through lwzu's pre-decrement.
        self.bind_label(load);
        let address = self.fresh_virtual_general();
        self.emit_address_high(address, &table);
        self.output.instructions.push(Instruction::ShiftLeftImmediate { a: GENERAL_SCRATCH, s: virtual_sig, shift: 2 });
        self.record_relocation(RelocationKind::Addr16Lo, &table);
        self.output.instructions.push(Instruction::AddImmediate { d: address, a: address, immediate: 0 });
        self.output.instructions.push(Instruction::Add { d: address, a: address, b: GENERAL_SCRATCH });
        self.output.instructions.push(Instruction::LoadWordWithUpdate { d: virtual_temp, a: address, offset: -4 });
        // STORE-IF: clear the slot through the updated base.
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: virtual_temp, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, skip_store);
        self.output.instructions.push(Instruction::load_immediate(GENERAL_SCRATCH, 0));
        self.output.instructions.push(Instruction::StoreWord { s: GENERAL_SCRATCH, a: address, offset: 0 });
        // GUARD3: the mixed ==||(&&) chain sharing one cold return block.
        self.bind_label(skip_store);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: virtual_temp, immediate: 1 });
        self.emit_branch_conditional_to(12, 2, return_zero);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: virtual_temp, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, after);
        self.output.instructions.push(Instruction::CompareWordImmediate { a: virtual_sig, immediate: 1 });
        self.emit_branch_conditional_to(4, 2, after);
        self.bind_label(return_zero);
        self.output.instructions.push(Instruction::load_immediate(result, 0));
        self.emit_branch_to(epilogue);
        // CALL-IF: branch over the exit call.
        self.bind_label(after);
        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: virtual_temp, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, call_label);
        self.output.instructions.push(Instruction::load_immediate(result, 0));
        self.record_relocation(RelocationKind::Rel24, &exit_name);
        self.output.instructions.push(Instruction::BranchAndLink { target: exit_name });
        // TAIL: the dispatch through ctr.
        self.bind_label(call_label);
        self.output.instructions.push(Instruction::move_register(12, virtual_temp));
        self.output.instructions.push(Instruction::move_register(result, virtual_sig));
        self.output.instructions.push(Instruction::MoveToCountRegister { s: 12 });
        self.output.instructions.push(Instruction::BranchToCountRegisterAndLink);
        self.output.instructions.push(Instruction::load_immediate(result, 0));
        self.bind_label(epilogue);
        self.output.anonymous_label_bump += 13;
        self.emit_epilogue_and_return();
        Ok(true)
    }

    pub(crate) fn emit_epilogue_and_return(&mut self) {
        let reload_saved_gprs = |generator: &mut Self| {
            for (index, &register) in generator.callee_saved.iter().enumerate() {
                let offset = generator.frame_size - 4 * (index as i16 + 1);
                generator.output.instructions.push(Instruction::LoadWord { d: register, a: 1, offset });
            }
        };
        if self.epilogue_lr_before_gprs && self.non_leaf {
            // Multi-pointer store sink: the saved LR reloads FIRST, then every callee-saved
            // GPR highest-first, then `mtlr` (`lwz r0,20; lwz r31,12; lwz r30,8; mtlr`).
            self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: self.frame_size + 4 });
            reload_saved_gprs(self);
            self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        } else if self.epilogue_lr_first && self.non_leaf {
            // Store-sink callee-saved: mwcc reloads all saved GPRs except the LOWEST, then
            // the saved LR, then the lowest GPR (count==1: `lwz r0; lwz r31`; count==2: `lwz
            // r31; lwz r0; lwz r30`). A register-death schedule this reproduces for one or
            // two saved values; three or more reschedule it (the sink restricts to <= 2).
            let last = self.callee_saved.len().saturating_sub(1);
            for (index, &register) in self.callee_saved.iter().enumerate() {
                if index == last {
                    continue;
                }
                let offset = self.frame_size - 4 * (index as i16 + 1);
                self.output.instructions.push(Instruction::LoadWord { d: register, a: 1, offset });
            }
            self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: self.frame_size + 4 });
            if let Some(&register) = self.callee_saved.last() {
                let offset = self.frame_size - 4 * (last as i16 + 1);
                self.output.instructions.push(Instruction::LoadWord { d: register, a: 1, offset });
            }
            self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
        } else {
            // Reload callee-saved registers (highest first, from the top of the frame)
            // before the saved-LR reload, so that reload stays directly before `mtlr`
            // where the hoist pass finds it and issues it right after the last call.
            reload_saved_gprs(self);
            if self.non_leaf {
                self.output.instructions.push(Instruction::LoadWord { d: 0, a: 1, offset: self.frame_size + 4 });
                self.output.instructions.push(Instruction::MoveToLinkRegister { s: 0 });
            }
        }
        if self.frame_size != 0 {
            self.output.instructions.push(Instruction::AddImmediate { d: 1, a: 1, immediate: self.frame_size });
        }
        self.output.instructions.push(Instruction::BranchToLinkRegister);
    }

    /// Emit a body statement.
    pub(crate) fn emit_statement(&mut self, statement: &Statement) -> Compilation<()> {
        match statement {
            Statement::Store { target, value } => self.emit_store(target, value),
            Statement::Expression(Expression::Call { name, arguments }) => {
                self.emit_call(name, arguments, None, false)
            }
            Statement::Expression(_) => Err(Diagnostic::error("only a call may be a bare statement (roadmap)")),
            // Reassignment is handled by value tracking; reaching here means it was
            // mixed with stores/calls, which that path defers.
            Statement::Assign { .. } => Err(Diagnostic::error("local reassignment mixed with stores/calls is not supported yet (roadmap)")),
            // The binary-search dispatch codegen is the next piece; switches parse
            // but defer for now (never miscompile).
            Statement::Switch { .. } => Err(Diagnostic::error("switch dispatch codegen is not implemented yet (roadmap)")),
            // A general if-statement (non-trailing, non-leaf, or with an else) needs
            // forward branches and basic-block scheduling — deferred for now.
            Statement::If { .. } => Err(Diagnostic::error("general if-statement codegen is not implemented yet (roadmap)")),
            // An early `return` inside the body needs early-return codegen (blr for
            // a leaf, a forward branch to the shared epilogue otherwise) — the
            // parser now models it, but the codegen is the next piece.
            Statement::Return(_) => Err(Diagnostic::error("early-return codegen is not implemented yet (roadmap)")),
            // Loops (while/do-while/for) parse but defer until the loop codegen
            // (backward branch + the callee-saved counter) lands.
            Statement::Loop { .. } => Err(Diagnostic::error("loop codegen is not implemented yet (roadmap)")),
        }
    }

    /// A trailing leaf `if (c) then; [else otherwise | else if …]` in a void
    /// function. With no else, the false path is a conditional return (the body
    /// then falls through to the function `blr`). With an else, branch over the
    /// then-body (and its `blr`) to the else, which is either a single statement
    /// or a nested trailing if (an `else if` chain). Each then-body is a single
    /// statement — multiple statements need the scheduler.
    fn emit_trailing_if(&mut self, condition: &Expression, then_body: &[Statement], else_body: &[Statement]) -> Compilation<()> {
        // `if (cond) tgt = c1; else tgt = c2;` — both arms a single constant store to the
        // same target — is one store of a select. mwcc branchless-ifies consecutive
        // constants (`srawi; addi`) and branch-materializes others into one register, then
        // stores once; route it through the conditional store path (byte-exact-or-defer)
        // rather than the two-branch form.
        if let ([Statement::Store { target: then_target, value: then_value }],
                [Statement::Store { target: else_target, value: else_value }]) = (then_body, else_body)
        {
            if same_operand(then_target, else_target)
                && constant_value(then_value).is_some()
                && constant_value(else_value).is_some()
            {
                let select = Expression::Conditional {
                    condition: Box::new(condition.clone()),
                    when_true: Box::new(then_value.clone()),
                    when_false: Box::new(else_value.clone()),
                };
                return self.emit_store(then_target, &select);
            }
        }
        // A no-else block of two-plus REGISTER-VALUED stores: the conditional return, then the
        // stores in source order. mwcc emits them sequentially — the values are already in
        // registers, so there is nothing to materialize or schedule (`cmpwi;beqlr;stw;stw;blr`). A
        // constant/global/computed store value needs the batch value scheduler and falls through.
        if then_body.len() > 1
            && else_body.is_empty()
            && then_body.iter().all(|statement| matches!(statement,
                Statement::Store { value: Expression::Variable(name), .. } if self.locations.contains_key(name.as_str())))
        {
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
            for statement in then_body {
                self.emit_statement(statement)?;
            }
            return Ok(());
        }
        if then_body.len() != 1 {
            return Err(Diagnostic::error("a multi-statement if-body needs the scheduler (roadmap)"));
        }
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        if else_body.is_empty() {
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
            return self.emit_statement(&then_body[0]);
        }
        // An `else if` chain keeps the two-exit form: the then-arm returns (`blr`), then
        // the nested trailing `if`.
        if let [Statement::If { condition: else_condition, then_body: else_then, else_body: else_else }] = else_body {
            let branch_index = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
            self.emit_statement(&then_body[0])?;
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            let label = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                *target = label;
            }
            return self.emit_trailing_if(else_condition, else_then, else_else);
        }
        if else_body.len() != 1 {
            return Err(Diagnostic::error("a multi-statement else-body needs the scheduler (roadmap)"));
        }
        // For a truthy condition (a bare register compare) with global-store arms, mwcc
        // uses the re-test idiom: the then-arm falls through to a *re-test* of the
        // condition and a conditional return, then the else — `cmpwi; beq L; A; L: cmpwi;
        // bnelr; B; blr`. A comparison condition re-tests by branchless recomputation (not
        // a second compare), and member/base-register arms keep the two-exit form; both of
        // those route to the two-exit branch below.
        let truthy = matches!(condition, Expression::Variable(_))
            || matches!(condition, Expression::Unary { operator: UnaryOperator::LogicalNot, operand } if matches!(operand.as_ref(), Expression::Variable(_)));
        let is_global_store = |statement: &Statement| {
            matches!(statement, Statement::Store { target: Expression::Variable(name), .. } if self.globals.contains_key(name.as_str()))
        };
        let use_retest = truthy && is_global_store(&then_body[0]) && is_global_store(&else_body[0]);
        let branch_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
        self.emit_statement(&then_body[0])?;
        if use_retest {
            let label = self.output.instructions.len();
            let (retest_options, retest_bit) = self.emit_condition_test(condition)?;
            self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: retest_options ^ 8, condition_bit: retest_bit });
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                *target = label;
            }
        } else {
            // Two-exit form: the then-arm returns, the conditional branch lands on the else.
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            let label = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                *target = label;
            }
        }
        self.emit_statement(&else_body[0])?;
        Ok(())
    }

    /// A non-trailing `if (c) { body }`: the false path branches forward over the
    /// body to the code that follows.
    fn emit_if_forward(&mut self, condition: &Expression, then_body: &[Statement]) -> Compilation<()> {
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let branch_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
        for statement in then_body {
            self.emit_statement(statement)?;
        }
        let label = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
            *target = label;
        }
        Ok(())
    }

    /// A leaf `if (c) { … return [v]; }` whose then-body ends in an early return:
    /// forward-branch over the body when the condition is false, emit the body
    /// (the `return` materializes the value and runs the epilogue — `blr` for a
    /// leaf), then patch the branch to land on the continuation (the rest of the
    /// function, which supplies the other exit).
    fn emit_if_early_return(&mut self, condition: &Expression, then_body: &[Statement], return_type: Type) -> Compilation<()> {
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let branch_index = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
        for statement in then_body {
            if let Statement::Return(value) = statement {
                if let Some(value) = value {
                    let result = match return_type {
                        Type::Float | Type::Double => Eabi::float_result().number,
                        _ => Eabi::general_result().number,
                    };
                    self.evaluate_tail(value, return_type, result)?;
                }
                self.emit_epilogue_and_return();
            } else {
                self.emit_statement(statement)?;
            }
        }
        let label = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
            *target = label;
        }
        Ok(())
    }

    /// A non-leaf function whose body begins with `if (c) { …calls…; return X; }`
    /// (the if is the first statement) followed by a straight-line continuation
    /// that supplies the other return. mwcc schedules the condition test into the
    /// prologue (between `mflr` and the LR store), the early return materializes X
    /// and branches to a SHARED epilogue, and the continuation falls into that same
    /// epilogue. Returns whether this path took over the whole body.
    fn try_non_leaf_if_first_early_return(&mut self, function: &Function) -> Compilation<bool> {
        // Shape: `if (c) { body…; return; } continuation…`, the if first, non-leaf,
        // no guards/locals, no else. The general/void return type only (a float
        // early return adds the FP result register — deferred).
        let [Statement::If { condition, then_body, else_body }, rest @ ..] = function.statements.as_slice() else {
            return Ok(false);
        };
        if !function_makes_call(function)
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || !else_body.is_empty()
            || matches!(function.return_type, Type::Float | Type::Double)
        {
            return Ok(false);
        }
        // The then-body must be straight-line calls/stores ending in the early
        // return; the continuation must likewise be straight-line (no nested
        // control flow, which would need its own branches).
        let Some((early_return, leading)) = then_body.split_last() else {
            return Ok(false);
        };
        let early_value = match early_return {
            Statement::Return(value) => value,
            _ => return Ok(false),
        };
        // Only calls may sit in the then-body or continuation: a call is a
        // scheduling barrier, so the value materialization that follows stays put.
        // A store would let mwcc's scheduler interleave the value into the store
        // sequence (`li r0,5; li r3,2; stw` rather than `li r0,5; stw; li r3,2`),
        // which this straight-line emission cannot reproduce — defer those.
        let call_only = |statement: &Statement| matches!(statement, Statement::Expression(_));
        if !leading.iter().all(call_only) || !rest.iter().all(call_only) {
            return Ok(false);
        }
        // A void function ends after its statements; a value-returning one supplies
        // the other exit through the trailing `return` expression. The early
        // return's value-ness must match (both void or both a value).
        let returns_value = function.return_type != Type::Void;
        if returns_value != early_value.is_some() || returns_value != function.return_expression.is_some() {
            return Ok(false);
        }
        // The condition test must be schedulable into the prologue: it cannot itself
        // make a call (that would need its own frame-aware sequencing).
        if expression_has_call(condition) {
            return Ok(false);
        }
        // A value computed AFTER a call on its path cannot be read from a
        // caller-saved register (the call clobbers it); mwcc would spill the source
        // to a callee-saved register (r31) and restructure the whole frame — that
        // is the next subsystem and is deferred. So a return value that follows a
        // call on its own path must be a compile-time constant (no register read).
        // The early return follows the then-body's calls; the continuation value
        // follows the continuation's calls (the false path skipped the then-body).
        let then_calls = leading.iter().any(statement_has_call);
        let rest_calls = rest.iter().any(statement_has_call);
        if then_calls && early_value.as_ref().is_some_and(|value| constant_value(value).is_none()) {
            return Ok(false);
        }
        if rest_calls && function.return_expression.as_ref().is_some_and(|value| constant_value(value).is_none()) {
            return Ok(false);
        }

        let result = Eabi::general_result().number;
        self.non_leaf = true;
        self.frame_size = 16;
        // The if's branch labels advance mwcc's anonymous-`@N` counter by 2.
        self.output.anonymous_label_bump = 2;
        self.output.instructions.push(Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -16 });
        self.output.instructions.push(Instruction::MoveFromLinkRegister { d: 0 });
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        self.output.instructions.push(Instruction::StoreWord { s: 0, a: 1, offset: 20 });
        // A BARE void early return (`if (a) return; g();`) has no then-body at all:
        // mwcc folds it to a single INVERTED conditional branch straight to the shared
        // epilogue — `bne EPILOGUE; bl g; EPILOGUE:` — rather than a skip over an
        // unconditional branch.
        if leading.is_empty() && early_value.is_none() {
            let epilogue_branch = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward {
                options: options ^ 8,
                condition_bit,
                target: 0,
            });
            for statement in rest {
                self.emit_statement(statement)?;
            }
            let epilogue_label = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[epilogue_branch] {
                *target = epilogue_label;
            }
            self.emit_epilogue_and_return();
            return Ok(true);
        }
        // False path skips the then-body to the continuation.
        let continuation_branch = self.output.instructions.len();
        self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
        // The then-body: the leading calls/stores, then the early return's value.
        for statement in leading {
            self.emit_statement(statement)?;
        }
        if let Some(value) = early_value {
            self.evaluate_tail(value, function.return_type, result)?;
        }
        // The early return branches to the shared epilogue. Reserve the slot — if
        // the continuation turns out to emit nothing (e.g. `return a` with `a`
        // already in the result register), mwcc lets the early return fall through
        // to the epilogue rather than branch, so the slot is dropped below.
        let branch_slot = self.output.instructions.len();
        self.output.instructions.push(Instruction::Branch { target: 0 });
        let continuation_label = self.output.instructions.len();
        for statement in rest {
            self.emit_statement(statement)?;
        }
        if let Some(return_expression) = &function.return_expression {
            self.evaluate_tail(return_expression, function.return_type, result)?;
        }
        if self.output.instructions.len() == continuation_label {
            // The continuation produced no instructions: the early return falls
            // straight through to the epilogue, and the false path targets the
            // epilogue directly. Drop the unnecessary branch.
            self.output.instructions.remove(branch_slot);
            let epilogue_label = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[continuation_branch] {
                *target = epilogue_label;
            }
        } else {
            // A non-empty continuation: the false path lands on it, and the early
            // return branches over it to the shared epilogue.
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[continuation_branch] {
                *target = continuation_label;
            }
            let epilogue_label = self.output.instructions.len();
            if let Instruction::Branch { target } = &mut self.output.instructions[branch_slot] {
                *target = epilogue_label;
            }
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// Emit a sequence of `if (c) return v;` guards followed by the final return.
    /// Each guard is its own block ending in `blr`; the last guard collapses the
    /// final return into a conditional return when the final value already sits in
    /// the result register.
    /// FLOAT PARAM REASSIGNMENT: `if (c) { x = -x; } return <expr of x>;` —
    /// the live float stays IN ITS PARAM REGISTER (an in-place fneg; measured,
    /// and `double t = x; if (c) t = -x;` canonicalizes identically). The
    /// bare-copy local aliases to the param when the param is otherwise dead.
    pub(crate) fn try_float_param_reassign(&mut self, function: &Function) -> Compilation<bool> {
        // The only "calls" allowed are the __fabs INTRINSIC in the arms
        // (a single fabs instruction, not a real call — checked per arm below).
        let has_real_call = function.return_expression.as_ref().is_some_and(crate::analysis::expression_has_call)
            || function.locals.iter().any(|local| local.initializer.as_ref().is_some_and(crate::analysis::expression_has_call));
        if !matches!(function.return_type, Type::Float | Type::Double)
            || function.return_expression.is_none()
            || !function.guards.is_empty()
            || has_real_call
            || self.behavior.global_addressing != GlobalAddressing::SmallData
        {
            return Ok(false);
        }
        // An optional single bare-copy local (`double t = x;`) aliases to the
        // param; more locals are outside this slice.
        let mut alias: Option<(&str, &str)> = None;
        match function.locals.as_slice() {
            [] => {}
            [local]
                if matches!(local.declared_type, Type::Float | Type::Double)
                    && !local.is_static
                    && local.array_length.is_none() =>
            {
                let Some(Expression::Variable(source)) = &local.initializer else { return Ok(false) };
                if self.float_register_of(source).is_err() {
                    return Ok(false);
                }
                alias = Some((local.name.as_str(), source.as_str()));
            }
            _ => return Ok(false),
        }
        fn resolve<'a>(alias: Option<(&'a str, &'a str)>, name: &'a str) -> &'a str {
            match alias {
                Some((local, source)) if local == name => source,
                _ => name,
            }
        }
        // Statements: `if (int-param cmp const) { fparam = -fparam; }` runs.
        let mut reassigns: Vec<(&Expression, &str, bool)> = Vec::new();
        for statement in &function.statements {
            let Statement::If { condition, then_body, else_body } = statement else { return Ok(false) };
            if !else_body.is_empty() || then_body.len() != 1 {
                return Ok(false);
            }
            let condition_ok = match condition {
                Expression::Variable(name) => self.lookup_general(name).is_some(),
                Expression::Binary { left, right, .. } => {
                    matches!(left.as_ref(), Expression::Variable(name) if self.lookup_general(name).is_some())
                        && constant_value(right).is_some()
                }
                _ => false,
            };
            if !condition_ok {
                return Ok(false);
            }
            let Statement::Assign { name, value } = &then_body[0] else { return Ok(false) };
            let target = resolve(alias, name);
            // `x = -x` (fneg) or `x = __fabs(x)` (the fabs instruction).
            let (source, is_abs) = match value {
                Expression::Unary { operator: UnaryOperator::Negate, operand } => match operand.as_ref() {
                    Expression::Variable(source) => (source, false),
                    _ => return Ok(false),
                },
                Expression::Call { name: callee, arguments } if is_intrinsic_call(callee) => match arguments.as_slice() {
                    [Expression::Variable(source)] => (source, true),
                    _ => return Ok(false),
                },
                _ => return Ok(false),
            };
            if resolve(alias, source) != target || self.float_register_of(target).is_err() {
                return Ok(false);
            }
            reassigns.push((condition, target, is_abs));
        }
        if reassigns.is_empty() {
            return Ok(false);
        }
        // The aliased param must not be read under its own name afterwards
        // (the alias takes the register over).
        let return_expression = function.return_expression.as_ref().expect("gated");
        if let Some((local, source)) = alias {
            if count_name_occurrences(return_expression, source) > 0 {
                return Ok(false);
            }
            let register = self.float_register_of(source).expect("checked");
            self.locations.insert(local.to_string(), crate::generator::Location {
                class: crate::generator::ValueClass::Float,
                register,
                signed: true,
                width: if function.return_type == Type::Float { 32 } else { 64 },
                pointee: None,
                stride: None,
            });
        }
        // Each if's join label advances mwcc's anonymous-@N counter by 2.
        self.output.anonymous_label_bump += 2 * reassigns.len() as u32;
        for (condition, target, is_abs) in &reassigns {
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            let branch_index = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
            let register = self.float_register_of(target).expect("checked");
            self.output.instructions.push(if *is_abs {
                Instruction::FloatAbsolute { d: register, b: register }
            } else {
                Instruction::FloatNegate { d: register, b: register }
            });
            let join = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                *target = join;
            }
        }
        let result = Eabi::float_result().number;
        self.evaluate_tail(return_expression, function.return_type, result)?;
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(true)
    }

    /// LIVE-ACROSS-BRANCHES: initialized int locals reassigned inside simple
    /// if-blocks, read after the joins (the s_atan `id`/`x` skeleton). The
    /// measured model: the condition's cmpwi leads; the inits compute
    /// SPECULATIVELY before the branch; every definition of one local shares
    /// ONE register home — r0 first unless a later use forbids it (an addi
    /// source), else the condition's DYING register, else a free volatile —
    /// and the trailing return/guards consume the locals as pseudo-params.
    pub(crate) fn try_live_across_branches(&mut self, function: &Function) -> Compilation<bool> {
        let int_return = function.return_type == Type::Int && function.return_expression.is_some();
        let void_stores = function.return_type == Type::Void && function.return_expression.is_none();
        if !(int_return || void_stores)
            || function_makes_call(function)
            || function.locals.is_empty()
            || self.behavior.global_addressing != GlobalAddressing::SmallData
        {
            return Ok(false);
        }
        if void_stores && !function.guards.is_empty() {
            return Ok(false);
        }
        // Trailing guards (`if (id < 0) return a;` — the id-tested-later form)
        // are allowed: their conditions/values may read the live locals, which
        // resolve through the registered home locations below.
        for guard in &function.guards {
            if !matches!(&guard.condition, Expression::Variable(_) | Expression::Binary { .. }) {
                return Ok(false);
            }
        }
        // Every local: int, initialized, non-static.
        if function.locals.iter().any(|local| {
            local.is_static
                || local.array_length.is_some()
                || local.initializer.is_none()
                || !matches!(local.declared_type, Type::Int | Type::UnsignedInt)
        }) {
            return Ok(false);
        }
        // The statements: a run of `if (param <cmp> const) { local = value; ... }`
        // blocks (no else), reassigning ONLY the declared locals.
        let local_names: Vec<&str> = function.locals.iter().map(|local| local.name.as_str()).collect();
        let simple_value = |expression: &Expression| -> bool {
            let readable = |name: &str| self.lookup_general(name).is_some() || local_names.contains(&name);
            match expression {
                Expression::IntegerLiteral(value) => i16::try_from(*value).is_ok(),
                Expression::Variable(name) => readable(name.as_str()),
                Expression::Binary { operator, left, right } => {
                    matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract | BinaryOperator::Multiply)
                        && matches!(left.as_ref(), Expression::Variable(name) if readable(name.as_str()))
                        && matches!(right.as_ref(), Expression::IntegerLiteral(value) if i16::try_from(*value).is_ok())
                }
                _ => false,
            }
        };
        // A VOID body: a run of ifs then TRAILING STORES to distinct SDA int
        // globals (the tail — DAG-scheduled below with the live locals as
        // pseudo-params).
        let mut tail_stores: Vec<&Statement> = Vec::new();
        let mut branch_conditions: Vec<&Expression> = Vec::new();
        for statement in &function.statements {
            if let Statement::Store { target, value } = statement {
                if !void_stores {
                    return Ok(false);
                }
                let Expression::Variable(global) = target else { return Ok(false) };
                if !matches!(self.globals.get(global.as_str()), Some(Type::Int | Type::UnsignedInt)) {
                    return Ok(false);
                }
                if !simple_value(value) {
                    return Ok(false);
                }
                tail_stores.push(statement);
                continue;
            }
            if !tail_stores.is_empty() {
                // A branch after the tail began — outside this slice.
                return Ok(false);
            }
            let Statement::If { condition, then_body, else_body } = statement else { return Ok(false) };
            if !else_body.is_empty() {
                return Ok(false);
            }
            // The condition: a bare param, or param <cmp> constant.
            let condition_param = match condition {
                Expression::Variable(name) => Some(name.as_str()),
                Expression::Binary { left, right, .. } => match (left.as_ref(), constant_value(right)) {
                    (Expression::Variable(name), Some(_)) => Some(name.as_str()),
                    _ => None,
                },
                _ => None,
            };
            let Some(condition_param) = condition_param else { return Ok(false) };
            if self.lookup_general(condition_param).is_none() || local_names.contains(&condition_param) {
                return Ok(false);
            }
            for inner in then_body {
                let Statement::Assign { name, value } = inner else { return Ok(false) };
                if !local_names.contains(&name.as_str()) || !simple_value(value) {
                    return Ok(false);
                }
            }
            branch_conditions.push(condition);
        }
        if branch_conditions.is_empty() || (void_stores && tail_stores.is_empty()) {
            return Ok(false);
        }
        // Init values must be simple too.
        for local in &function.locals {
            if !simple_value(local.initializer.as_ref().expect("gated")) {
                return Ok(false);
            }
        }
        // HOME SELECTION. A use as an addi source forbids r0: an init/arm value
        // `local <op> const` reading the local, the return expression adding a
        // constant to it, or a tail store's value doing the same.
        let forbids_r0 = |name: &str| -> bool {
            let reads_as_addi = |expression: &Expression| -> bool {
                matches!(expression, Expression::Binary { operator: BinaryOperator::Add | BinaryOperator::Subtract, left, right }
                    if matches!(left.as_ref(), Expression::Variable(inner) if inner == name) && constant_value(right).is_some())
            };
            if function.return_expression.as_ref().is_some_and(&reads_as_addi) {
                return true;
            }
            if tail_stores.iter().any(|statement| matches!(statement, Statement::Store { value, .. } if reads_as_addi(value))) {
                return true;
            }
            function.statements.iter().any(|statement| {
                let Statement::If { then_body, .. } = statement else { return false };
                then_body.iter().any(|inner| matches!(inner, Statement::Assign { value, .. } if reads_as_addi(value)))
            })
        };
        // Dying condition registers: a condition param never referenced later.
        let mut dying_condition_registers: Vec<u8> = Vec::new();
        for condition in &branch_conditions {
            let param = match condition {
                Expression::Variable(name) => name.as_str(),
                Expression::Binary { left, .. } => match left.as_ref() {
                    Expression::Variable(name) => name.as_str(),
                    _ => continue,
                },
                _ => continue,
            };
            let uses_elsewhere = function.return_expression.as_ref().map_or(0, |expression| count_name_occurrences(expression, param))
                + function
                    .statements
                    .iter()
                    .map(|statement| statement_reads(statement, param))
                    .sum::<usize>()
                > branch_conditions
                    .iter()
                    .filter(|other| {
                        matches!(other, Expression::Variable(name) if name == param)
                            || matches!(other, Expression::Binary { left, .. } if matches!(left.as_ref(), Expression::Variable(name) if name == param))
                    })
                    .count();
            if !uses_elsewhere {
                if let Some(register) = self.lookup_general(param) {
                    dying_condition_registers.push(register);
                }
            }
        }
        let mut homes: Vec<(String, u8)> = Vec::new();
        let mut taken: Vec<u8> = Vec::new();
        for local in &function.locals {
            // In a VOID body, r0 belongs to the LAST tail chain: the local may
            // take it only when it IS that chain's value (stored bare by the
            // final store, read nowhere else in the tail).
            let r0_ok = if void_stores {
                let last_is_bare_self = matches!(
                    tail_stores.last(),
                    Some(Statement::Store { value: Expression::Variable(name), .. }) if *name == local.name
                );
                let tail_reads: usize = tail_stores
                    .iter()
                    .map(|statement| statement_reads(statement, &local.name))
                    .sum();
                last_is_bare_self && tail_reads == 1 && !forbids_r0(&local.name)
            } else {
                !forbids_r0(&local.name)
            };
            let candidates: Vec<u8> = if !r0_ok {
                dying_condition_registers.iter().copied().chain(5..=12).collect()
            } else {
                std::iter::once(0u8).chain(dying_condition_registers.iter().copied()).chain(5..=12).collect()
            };
            let Some(register) = candidates.into_iter().find(|register| !taken.contains(register)) else {
                return Ok(false);
            };
            taken.push(register);
            homes.push((local.name.clone(), register));
        }
        let home_of = |name: &str| homes.iter().find(|(local, _)| local == name).map(|&(_, register)| register);

        // EMISSION. First branch: cmpwi, speculative inits, branch; later
        // branches: cmpwi, branch, arm. Each if's join label advances mwcc's
        // anonymous-@N counter by 2.
        self.output.anonymous_label_bump += 2 * branch_conditions.len() as u32;
        for (index, statement) in function.statements.iter().enumerate() {
            // Tail stores emit after the branch structure.
            let Statement::If { condition, then_body, .. } = statement else { break };
            let (options, condition_bit) = self.emit_condition_test(condition)?;
            if index == 0 {
                for local in &function.locals {
                    let home = home_of(&local.name).expect("assigned");
                    self.evaluate(local.initializer.as_ref().expect("gated"), Type::Int, home)?;
                }
            }
            let branch_index = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
            for inner in then_body {
                let Statement::Assign { name, value } = inner else { unreachable!() };
                // A reassignment may read the local itself (its home).
                let home = home_of(name).expect("assigned");
                self.evaluate_with_live_locals(value, home, &homes)?;
            }
            let join = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                *target = join;
            }
        }
        // The trailing return consumes the locals as pseudo-params.
        for (name, register) in &homes {
            self.locations.insert(name.clone(), crate::generator::Location {
                class: crate::generator::ValueClass::General,
                register: *register,
                signed: true,
                width: 32,
                pointee: None,
                stride: None,
            });
        }
        if void_stores {
            // The tail: a single bare-local store emits directly; a richer run
            // routes through the DAG store-fill with the live locals as
            // PSEUDO-PARAMS (their homes registered above resolve through
            // lookup_general like any parameter).
            if let [Statement::Store { target: Expression::Variable(global), value: Expression::Variable(name) }] =
                tail_stores.as_slice()
            {
                let source = self.lookup_general(name).expect("registered home");
                self.record_relocation(RelocationKind::EmbSda21, global);
                self.output.instructions.push(Instruction::StoreWord { s: source, a: 0, offset: 0 });
                self.emit_epilogue_and_return();
                return Ok(true);
            }
            let mut pseudo = function.parameters.clone();
            for (name, _) in &homes {
                pseudo.push(mwcc_syntax_trees::Parameter { parameter_type: Type::Int, name: name.clone() });
            }
            let synthesized = Function {
                return_type: Type::Void,
                name: function.name.clone(),
                is_static: function.is_static,
                is_weak: function.is_weak,
                parameters: pseudo,
                locals: Vec::new(),
                statements: tail_stores.iter().map(|&statement| statement.clone()).collect(),
                guards: Vec::new(),
                return_expression: None,
            };
            if !self.try_dag_store_fill(&synthesized)? {
                return Err(Diagnostic::error("a live-across store tail outside the DAG envelope needs more vocabulary (roadmap)"));
            }
            return Ok(true);
        }
        let return_expression = function.return_expression.as_ref().expect("gated");
        let result = Eabi::general_result().number;
        if function.guards.is_empty() {
            self.evaluate_tail(return_expression, Type::Int, result)?;
            self.output.instructions.push(Instruction::BranchToLinkRegister);
        } else {
            self.emit_guard_sequence(&function.guards, return_expression, Type::Int, result)?;
        }
        Ok(true)
    }

    /// evaluate() with the live-local homes visible as locations (a
    /// reassignment reads its own or a sibling's home).
    fn evaluate_with_live_locals(&mut self, value: &Expression, destination: u8, homes: &[(String, u8)]) -> Compilation<()> {
        for (name, register) in homes {
            self.locations.entry(name.clone()).or_insert(crate::generator::Location {
                class: crate::generator::ValueClass::General,
                register: *register,
                signed: true,
                width: 32,
                pointee: None,
                stride: None,
            });
        }
        self.evaluate(value, Type::Int, destination)
    }

    pub(crate) fn emit_guard_sequence(
        &mut self,
        guards: &[GuardedReturn],
        final_return: &Expression,
        return_type: Type,
        result: u8,
    ) -> Compilation<()> {
        let final_in_result = match final_return {
            Expression::Variable(name) => self.locations.get(name).map(|location| location.register) == Some(result),
            _ => false,
        };

        // mwcc reuses one `cmpwi` across consecutive guards that test the same operand against the
        // same constant: `if (a < 0) ...; if (a == 0) ...` shares `cmpwi r3,0`, the second guard
        // branching on the same result (`bne`). That cross-guard condition-register reuse is not
        // modeled — each guard here emits its own compare — so a sequence containing such a pair
        // would emit a redundant second `cmpwi` (a byte diff). Defer it rather than ship that.
        let guard_count = guards.len();
        for (pair_index, pair) in guards.windows(2).enumerate() {
            if let (Some(first), Some(second)) =
                (guard_comparison_key(&pair[0].condition), guard_comparison_key(&pair[1].condition))
            {
                if first == second {
                    // When the SECOND guard of the pair is the LAST guard, it folds with the final
                    // return into a select (the `is_last` path below). If that select lowers
                    // branchlessly (sign-mask `srawi`/`srwi`, or a consecutive-constant sign select)
                    // it emits NO compare, so the shared key produces no redundant compare and no
                    // cross-guard CR reuse is needed — mwcc emits one compare for the earlier guard
                    // and the branchless tail (e.g. `if(a>0)return 1; if(a<0)return -1; return 0;` ->
                    // `cmpwi;ble;li 1;blr; srawi;blr`; or a `> 0 ? 2 : 3` tail -> `neg;andc;srawi;
                    // addi`). Compare-based tails (==0/!=0/<=0/variable) are NOT branchless here and
                    // keep deferring (they also defer in evaluate_tail), so no DIFF is shipped.
                    let second_is_last = pair_index + 2 == guard_count;
                    if second_is_last && (!final_in_result || constant_value(&pair[1].value).is_some()) {
                        let select = guard_select(&pair[1].condition, &pair[1].value, final_return);
                        if let Expression::Conditional { condition, when_true, when_false } = &select {
                            if crate::control_flow::select_folds_branchless(condition, when_true, when_false) {
                                continue;
                            }
                        }
                    }
                    return Err(Diagnostic::error("consecutive guards sharing a compare need cross-guard CR reuse (roadmap)"));
                }
            }
        }

        for (index, guard) in guards.iter().enumerate() {
            let is_last = index + 1 == guards.len();

            // A null-guarded dereference `if (!p) return CONST; return *p;` cannot fold branchless
            // (dereferencing null is unsafe); mwcc emits a real branch with the deref in the
            // fall-through and the constant as the cold tail: `cmplwi p,0; beq COLD; <*p>; blr;
            // COLD: li CONST; blr`.
            if is_last {
                if let Some((pointer, hot, cold)) = guarded_null_dereference(&guard.condition, &guard.value, final_return, return_type) {
                    if let Some(pointer_register) = self.lookup_general(pointer) {
                        self.output.instructions.push(Instruction::CompareLogicalWordImmediate { a: pointer_register, immediate: 0 });
                        let branch_index = self.output.instructions.len();
                        self.output.instructions.push(Instruction::BranchConditionalForward { options: 12, condition_bit: 2, target: 0 });
                        self.evaluate_tail(hot, return_type, result)?;
                        self.output.instructions.push(Instruction::BranchToLinkRegister);
                        let cold_label = self.output.instructions.len();
                        if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                            *target = cold_label;
                        }
                        self.evaluate_tail(cold, return_type, result)?;
                        self.output.instructions.push(Instruction::BranchToLinkRegister);
                        return Ok(());
                    }
                }
            }

            // mwcc compiles the final guard together with the fall-through return as
            // one branchless select `(cond) ? value : final` — the same form as a
            // lone guard — not a third early-return branch. Earlier guards stay as
            // forward-branching early returns.
            // The last guard folds into the fall-through as a single select `(cond) ? value :
            // final` whenever the final isn't already in the result register, OR the guard value
            // is a constant (the select's constant-arm forms cover `(a>10) ? 1 : a` etc., which
            // the in-result `bnelr` path below cannot — it needs a register value).
            if is_last && (!final_in_result || constant_value(&guard.value).is_some()) {
                let select = guard_select(&guard.condition, &guard.value, final_return);
                // ATTEMPT the select; when its lowering has no vocabulary for
                // the fall-through (a table load, a cast) mwcc uses a real
                // early-return BRANCH instead (measured: `cmpwi;bne;li;blr;
                // <table>;blr` for the ctype shape) — roll back and continue
                // the loop, which emits the guard as an early return and the
                // final via the fall-through below.
                let instructions_before = self.output.instructions.len();
                let relocations_before = self.output.relocations.len();
                let virtuals_before = self.next_virtual;
                let bump_before = self.output.anonymous_label_bump;
                match self.evaluate_tail(&select, return_type, result) {
                    Ok(()) => {
                        self.output.instructions.push(Instruction::BranchToLinkRegister);
                        return Ok(());
                    }
                    Err(_) => {
                        self.output.instructions.truncate(instructions_before);
                        self.output.relocations.truncate(relocations_before);
                        self.next_virtual = virtuals_before;
                        self.output.anonymous_label_bump = bump_before;
                    }
                }
            }

            let (options, condition_bit) = self.emit_condition_test(&guard.condition)?;
            // A (non-last) guard with a CONSTANT value: forward-branch past the return when the
            // condition is false, load the constant into the result, and return —
            // `cmpwi; bge skip; li result, c; blr; skip:`. (A constant has no leaf register, so the
            // leaf paths below would defer at general_register_of_leaf.)
            if let Some(constant) = constant_value(&guard.value) {
                let branch_index = self.output.instructions.len();
                self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
                self.load_integer_constant(result, constant);
                self.output.instructions.push(Instruction::BranchToLinkRegister);
                let next = self.output.instructions.len();
                if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                    *target = next;
                }
                continue;
            }
            let value_register = self.general_register_of_leaf(&guard.value)?;

            if is_last && final_in_result {
                // false path returns the final value already in the result register
                self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options, condition_bit });
                if result != value_register {
                    self.output.instructions.push(Instruction::move_register(result, value_register));
                }
                self.output.instructions.push(Instruction::BranchToLinkRegister);
                return Ok(());
            }

            // A non-last guard whose value already sits in the result register is a
            // conditional return falling through to the next guard (mwcc: `cmpwi; bnelr`),
            // not a forward branch over the return.
            if result == value_register {
                self.output.instructions.push(Instruction::BranchConditionalToLinkRegister { options: options ^ 8, condition_bit });
                continue;
            }
            let branch_index = self.output.instructions.len();
            self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
            self.output.instructions.push(Instruction::move_register(result, value_register));
            self.output.instructions.push(Instruction::BranchToLinkRegister);
            let next = self.output.instructions.len();
            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                *target = next;
            }
        }

        // Final fall-through return.
        self.evaluate_tail(final_return, return_type, result)?;
        self.output.instructions.push(Instruction::BranchToLinkRegister);
        Ok(())
    }

    /// Evaluate the function result. A conditional in this tail position can use a
    /// conditional return when one of its values already sits in the result register.
    pub(crate) fn evaluate_tail(&mut self, expression: &Expression, value_type: Type, result: u8) -> Compilation<()> {
        match expression {
            Expression::Conditional { condition, when_true, when_false } => match value_type {
                Type::Float | Type::Double => self.emit_float_conditional(condition, when_true, when_false, result, true),
                _ => {
                    // ATTEMPT the select; a false-arm outside its vocabulary
                    // (a table load) uses mwcc's early-return BRANCH — the
                    // ternary is the guard form `if (cond) return T; return F`
                    // (measured on the ctype tolower shape).
                    let instructions_before = self.output.instructions.len();
                    let relocations_before = self.output.relocations.len();
                    let virtuals_before = self.next_virtual;
                    let bump_before = self.output.anonymous_label_bump;
                    match self.emit_conditional(condition, when_true, when_false, result, true) {
                        Ok(()) => Ok(()),
                        Err(error) => {
                            self.output.instructions.truncate(instructions_before);
                            self.output.relocations.truncate(relocations_before);
                            self.next_virtual = virtuals_before;
                            self.output.anonymous_label_bump = bump_before;
                            // Emit the branch form DIRECTLY (a nested-ternary
                            // fall-through would recurse through the same
                            // fallback forever — defer that).
                            let Some(constant) = constant_value(when_true) else { return Err(error) };
                            if matches!(when_false.as_ref(), Expression::Conditional { .. }) {
                                return Err(error);
                            }
                            let (options, condition_bit) = self.emit_condition_test(condition)?;
                            let branch_index = self.output.instructions.len();
                            self.output.instructions.push(Instruction::BranchConditionalForward { options, condition_bit, target: 0 });
                            self.load_integer_constant(result, constant);
                            self.output.instructions.push(Instruction::BranchToLinkRegister);
                            let next = self.output.instructions.len();
                            if let Instruction::BranchConditionalForward { target, .. } = &mut self.output.instructions[branch_index] {
                                *target = next;
                            }
                            self.evaluate_tail(when_false, value_type, result)
                        }
                    }
                }
            },
            Expression::Binary { operator: operator @ (BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr), left, right } => {
                self.emit_short_circuit(*operator, left, right, result)
            }
            // De Morgan: `return !(X && Y)` is `!X || !Y` and `!(X || Y)` is `!X && !Y` —
            // mwcc folds the negation into the short-circuit exits rather than computing the
            // operator into a register and inverting it (cntlzw/srwi). Single level only;
            // a nested logical operand defers to the general path.
            Expression::Unary { operator: UnaryOperator::LogicalNot, operand }
                if matches!(operand.as_ref(), Expression::Binary { operator: BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr, .. }) =>
            {
                let Expression::Binary { operator: inner, left, right } = operand.as_ref() else { unreachable!() };
                let is_logical = |expression: &Expression| {
                    matches!(expression, Expression::Binary { operator: BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr, .. })
                };
                if is_logical(left.as_ref()) || is_logical(right.as_ref()) {
                    return Err(Diagnostic::error("a nested negated logical needs the general short-circuit (roadmap)"));
                }
                let flipped = if *inner == BinaryOperator::LogicalAnd { BinaryOperator::LogicalOr } else { BinaryOperator::LogicalAnd };
                let not_left = Expression::Unary { operator: UnaryOperator::LogicalNot, operand: Box::new(left.as_ref().clone()) };
                let not_right = Expression::Unary { operator: UnaryOperator::LogicalNot, operand: Box::new(right.as_ref().clone()) };
                self.emit_short_circuit(flipped, &not_left, &not_right, result)
            }
            // A narrow return type truncates the returned value. A `(type)` cast
            // expression already yields the narrow type, so it falls through to the
            // normal path; everything else is coerced here.
            other if is_narrow_int(value_type) && !matches!(other, Expression::Cast { .. }) => {
                self.evaluate_narrow_return(other, value_type, result)
            }
            other => self.evaluate(other, value_type, result),
        }
    }

    pub(crate) fn evaluate_single_local(
        &mut self,
        local: &LocalDeclaration,
        return_expression: &Expression,
        return_type: Type,
        result: u8,
    ) -> Compilation<()> {
        let class = class_of(local.declared_type)?;
        // The single-local straight-line path needs the local's initializer; an
        // uninitialized local (its value comes from an assignment) is value-tracked.
        let initializer = local
            .initializer
            .as_ref()
            .ok_or_else(|| Diagnostic::error("an uninitialized single local is not supported here (roadmap)"))?;

        // `return x;` — the local is the result, so compute its initializer
        // straight into the result register.
        if matches!(return_expression, Expression::Variable(name) if *name == local.name) {
            // A signed narrow local (char/short) returned at a wider type must be
            // sign-extended — `char c = *s; return c;` is `lbz; extsb` like the direct
            // `return *s`. Evaluating the initializer at the local's own narrow type drops
            // that widening, and whether the value is already extended depends on the
            // initializer (a global narrow load appends extsb/lha; a char* deref's `lbz`
            // and a parameter leave the raw byte). Defer the not-already-extended cases
            // rather than return a zero-extended byte where a sign-extended char is meant.
            if self.signed_of(local.declared_type)
                && local.declared_type.width() < return_type.width()
                && local.declared_type.width() < 32
            {
                let initializer_extends = match initializer {
                    // A global signed-narrow load appends the extension (lbz+extsb / lha).
                    Expression::Variable(name) => self.globals.contains_key(name.as_str()),
                    // `lha` sign-extends a halfword; `lbz` does not extend a byte.
                    Expression::Dereference { .. } | Expression::Index { .. } | Expression::Member { .. } => {
                        local.declared_type.width() >= 16
                    }
                    _ => false,
                };
                if !initializer_extends {
                    return Err(Diagnostic::error("a signed narrow local returned at a wider type needs a widening coercion (roadmap)"));
                }
            }
            // A NARROWING leaf initializer — `char c = a;` for a wider `a` — truncates to the
            // narrow type. Inlining it into the return drops that truncation (and the char
            // return's sign-extension): mwcc emits `extsb r3,r3` for `char f(int a){ char c =
            // a; return c; }`, ours returned the raw int. Defer the narrowing.
            if local.declared_type.width() < 32 {
                if let Ok((_, init_width, _)) = self.leaf_info(initializer) {
                    if init_width as u32 > local.declared_type.width() as u32 {
                        return Err(Diagnostic::error("a narrowing narrow local (char/short from a wider value) returned is not supported yet (roadmap)"));
                    }
                }
            }
            return self.evaluate(initializer, local.declared_type, result);
        }

        // An additively-defined local used as an operand of an addition
        // (`int t = a + b; return t + c;`) is one mwcc keeps in a register and
        // mutates in place (`add r3,r3,r4; add r3,r3,r5`); our leaf-in-scratch
        // lowering would instead reassociate it like a direct sum. Defer that exact
        // shape (a `+`-init local feeding a `+`); the allocator will later make it
        // byte-exact. Other shapes (`*` init, or a `*`/`-` use) already match.
        fn feeds_an_addition(name: &str, expression: &Expression) -> bool {
            let is_local = |operand: &Expression| matches!(operand, Expression::Variable(variable) if variable == name);
            match expression {
                Expression::AggregateLiteral(_) => false,
                Expression::PostStep { target, .. } => feeds_an_addition(name, target),
                Expression::Binary { operator, left, right } => {
                    (*operator == BinaryOperator::Add && (is_local(left) || is_local(right)))
                        || feeds_an_addition(name, left)
                        || feeds_an_addition(name, right)
                }
                Expression::Unary { operand, .. } | Expression::Cast { operand, .. } | Expression::AddressOf { operand } => feeds_an_addition(name, operand),
                Expression::Conditional { condition, when_true, when_false } => {
                    feeds_an_addition(name, condition) || feeds_an_addition(name, when_true) || feeds_an_addition(name, when_false)
                }
                Expression::Dereference { pointer } => feeds_an_addition(name, pointer),
                Expression::Index { base, index } => feeds_an_addition(name, base) || feeds_an_addition(name, index),
                Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => feeds_an_addition(name, base),
                Expression::Assign { target, value } => feeds_an_addition(name, target) || feeds_an_addition(name, value),
                Expression::Comma { left, right } => feeds_an_addition(name, left) || feeds_an_addition(name, right),
                Expression::Call { arguments, .. } => arguments.iter().any(|argument| feeds_an_addition(name, argument)),
                Expression::Variable(_) | Expression::IntegerLiteral(_) | Expression::FloatLiteral(_) | Expression::StringLiteral(_) => false,
            }
        }
        if matches!(initializer, Expression::Binary { operator: BinaryOperator::Add, .. })
            && feeds_an_addition(&local.name, return_expression)
        {
            return Err(Diagnostic::error("an additively-defined local used in a sum needs the register allocator to match mwcc's in-place mutation (roadmap)"));
        }

        // Otherwise the local lives in the scratch register and is used as a leaf.
        // That only works if the result expression does not itself need the scratch.
        if needs_scratch(return_expression) {
            return Err(Diagnostic::error("local reused inside a scratch-needing expression (roadmap M1)"));
        }
        let scratch = match class {
            ValueClass::General => GENERAL_SCRATCH,
            ValueClass::Float => FLOAT_SCRATCH,
        };
        self.evaluate(initializer, local.declared_type, scratch)?;
        let signed = self.signed_of(local.declared_type);
        let pointee = match local.declared_type {
            Type::Pointer(pointee) => Some(pointee),
            _ => None,
        };
        let stride = pointer_stride(local.declared_type);
        self.locations.insert(local.name.clone(), Location { class, register: scratch, signed, width: local.declared_type.width(), pointee, stride });
        self.evaluate(return_expression, return_type, result)
    }

    pub(crate) fn evaluate(&mut self, expression: &Expression, value_type: Type, destination: u8) -> Compilation<()> {
        // An `(int)` cast of an UNSIGNED-narrow or int-typed operand is a no-op
        // (the lbz/lhz load already zero-extends): unwrap it. A signed-narrow
        // operand keeps the cast (its widening is the extsb/extsh the inner
        // paths model).
        if let (Type::Int | Type::UnsignedInt, Expression::Cast { target_type: Type::Int | Type::UnsignedInt, operand }) =
            (value_type, expression)
        {
            let element = match operand.as_ref() {
                Expression::Index { base, .. } => match base.as_ref() {
                    Expression::Variable(name) => self.globals.get(name.as_str()).copied(),
                    _ => None,
                },
                _ => None,
            };
            match element {
                // An UNSIGNED narrow (or int) element zero-extends in its own
                // load (lbzx/lhzx): the cast is a no-op.
                Some(Type::UnsignedChar | Type::UnsignedShort | Type::Int | Type::UnsignedInt) => {
                    return self.evaluate(operand, value_type, destination);
                }
                // A SIGNED narrow element's widening (lbzx then extsb) is the
                // Index path's own job — the cast is a no-op wrapper here too.
                Some(Type::Char | Type::Short) => {
                    return self.evaluate(operand, value_type, destination);
                }
                _ => {}
            }
            if matches!(operand.as_ref(), Expression::Variable(_) | Expression::IntegerLiteral(_) | Expression::Binary { .. }) {
                return self.evaluate(operand, value_type, destination);
            }
        }
        match value_type {
            // A `double` shares the FPR file with `float`; the float path picks the
            // double-precision instructions via is_double_value. An integer leaf in
            // a float context is an implicit int->float conversion (the same magic-
            // constant sequence as the explicit `(float)`/`(double)` cast).
            Type::Float | Type::Double => {
                // A bare float literal materializes at the CONTEXT precision: an 8-byte
                // pooled `lfd` for a double, the rounded 4-byte `lfs` for a float.
                // evaluate_float cannot know the context and always picked single,
                // which mis-typed every double-constant return (`return 0.0;`).
                if let Expression::FloatLiteral(value) = expression {
                    self.load_float_literal(destination, *value, value_type == Type::Double);
                    return Ok(());
                }
                if self.is_integer_leaf(expression) {
                    return self.emit_cast_to_float(expression, destination, value_type == Type::Double);
                }
                // A call returning int — or an implicitly-declared callee (defaults to int),
                // the libm `w_*` wrappers `double acos(double x){ return __ieee754_acos(x); }`
                // — leaves its result in r3. Convert it to the CONTEXT precision (this branch
                // knows `value_type`, which evaluate_float does not) via the magic-bias
                // sequence, reusing the non-leaf call prologue's frame (no second stwu). mwcc
                // schedules the call-result conversion value-store-first: the call->xoris->stw
                // value chain is the critical path, so the independent bias load fills the slot
                // after. An intrinsic (`__fabs`) is not a real call and is left to evaluate_float.
                if let Expression::Call { name, arguments } = expression {
                    if !is_intrinsic_call(name) && !matches!(self.call_return_types.get(name), Some(Type::Float | Type::Double)) {
                        let source = Eabi::general_result().number;
                        self.emit_call(name, arguments, None, false)?;
                        let bias_register = if destination != FLOAT_SCRATCH { destination } else { Eabi::float_result().number };
                        self.emit_int_to_float_body(source, destination, value_type == Type::Double, true, bias_register, true);
                        return Ok(());
                    }
                }
                // An integer memory load (`*p`, `a[i]`, `s.member` of integer type) in a
                // float context needs the loaded value run through the int->float conversion.
                // That path is not wired, so defer rather than hand it to evaluate_float,
                // which would mis-evaluate the integer as a float and load it into the GPR
                // whose NUMBER matches the float destination (f1 -> r1, clobbering the stack
                // pointer). Float-typed loads fall through to evaluate_float as before.
                // A deref/index of a leaf-variable base (int pointer, int global array) whose
                // loaded value is not float, or a direct integer struct member. Member-based
                // bases (`*p->fq`, `p->e[i]`) are left to evaluate_float — is_float_value
                // cannot resolve them, and those float loads are already byte-exact.
                let integer_memory_load = match expression {
                    Expression::Dereference { pointer } => {
                        matches!(pointer.as_ref(), Expression::Variable(_)) && !self.is_float_value(expression)
                    }
                    Expression::Index { base, .. } => {
                        matches!(base.as_ref(), Expression::Variable(_)) && !self.is_float_value(expression)
                    }
                    Expression::Member { member_type, .. } => !matches!(member_type, Type::Float | Type::Double),
                    // A plain file-scope global of INT (non-float) type read in a float context —
                    // `double f(){ return gi; }` — is an integer memory load too. Without this,
                    // evaluate_float treats it as a float global and loads it (`lwz`) into the GPR
                    // whose number matches the float destination: f1 -> r1, CLOBBERING the stack
                    // pointer. A local/param is not a memory load (excluded via `locations`).
                    Expression::Variable(name) => {
                        !self.locations.contains_key(name.as_str())
                            && matches!(self.globals.get(name.as_str()), Some(global_type) if !matches!(global_type, Type::Float | Type::Double))
                    }
                    _ => false,
                };
                if integer_memory_load {
                    return Err(Diagnostic::error("an integer memory load in a float context needs an int->float conversion (roadmap)"));
                }
                self.evaluate_float(expression, destination)
            }
            Type::Void => Err(Diagnostic::error("cannot evaluate a void expression")),
            // A float leaf in an integer context is an implicit float->int conversion
            // (the same `fctiwz` + frame bounce as the explicit `(int)` cast).
            _ => {
                if self.is_float_value(expression) {
                    return self.emit_cast_to_integer(value_type, expression, destination);
                }
                // A whole signed-`char` load promoted to `int` sign-extends the
                // loaded byte: `lbz d,…; extsb d,d`. (`lbz` zero-extends, so the
                // promotion needs the trailing `extsb`; the narrow-return path
                // calls `evaluate_general` directly and so keeps the bare `lbz`.)
                if matches!(value_type, Type::Int | Type::UnsignedInt) && self.is_signed_byte_load(expression)? {
                    self.evaluate_general(expression, destination)?;
                    self.emit_widen(destination, destination, 8, true);
                    return Ok(());
                }
                self.evaluate_general(expression, destination)
            }
        }
    }

    /// Whether `expression` is a full-width integer leaf variable (an int/unsigned
    /// in a GPR, not a pointer or a narrow type) — the operand an implicit
    /// int->float conversion accepts.
    fn is_integer_leaf(&self, expression: &Expression) -> bool {
        matches!(expression, Expression::Variable(name)
            if self.locations.get(name.as_str())
                .is_some_and(|location| location.class == ValueClass::General && location.width == 32 && location.pointee.is_none()))
    }
}






/// Whether any statement, guard, or the return expression calls one of `names`.
fn function_calls_any(function: &Function, names: &std::collections::HashSet<String>) -> bool {
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
                    || default.as_ref().is_some_and(|expression| expression_calls(expression, names))
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





