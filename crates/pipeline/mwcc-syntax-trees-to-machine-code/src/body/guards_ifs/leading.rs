//! Leading-guard hoist: move order-independent leading guards into function.guards.

#[allow(unused_imports)]
use super::*;

impl Generator {
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
    pub(crate) fn hoist_order_independent_leading_guards(
        &self,
        function: &Function,
    ) -> Option<Function> {
        // GC/1.2.5n preserves the source-order early-return diamond.  Its later
        // integer-select pipeline does not perform the guard/tail folds modeled by
        // value tracking, even when moving the guard would be semantically safe.
        let legacy_source_order_guard = matches!(
            function.statements.first(),
            Some(Statement::If {
                condition,
                then_body,
                else_body,
            }) if else_body.is_empty()
                && matches!(then_body.as_slice(), [Statement::Return(Some(value))]
                    if matches!(value, Expression::Variable(_))
                        || !matches!(condition, Expression::Variable(_)))
        );
        if self.behavior.integer_select_style
            == mwcc_versions::IntegerSelectStyle::BranchPreserving
            && legacy_source_order_guard
        {
            return None;
        }
        if !matches!(function.statements.first(), Some(Statement::If { .. })) {
            return None;
        }
        let mut hoisted: Vec<GuardedReturn> = Vec::new();
        let mut rest: Vec<Statement> = Vec::new();
        let mut in_prefix = true;
        for statement in &function.statements {
            if in_prefix {
                if let Statement::If {
                    condition,
                    then_body,
                    else_body,
                } = statement
                {
                    if else_body.is_empty() {
                        if let [Statement::Return(Some(value))] = then_body.as_slice() {
                            hoisted.push(GuardedReturn {
                                condition: condition.clone(),
                                value: value.clone(),
                            });
                            continue;
                        }
                    }
                }
                in_prefix = false;
            }
            rest.push(statement.clone());
        }
        if hoisted.is_empty()
            || !rest
                .iter()
                .all(|statement| matches!(statement, Statement::Assign { .. }))
        {
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
        let reads_written = |expression: &Expression| {
            written
                .iter()
                .any(|name| expression_reads_name(expression, name))
        };
        if hoisted
            .iter()
            .any(|guard| reads_written(&guard.condition) || reads_written(&guard.value))
        {
            return None;
        }
        // A guard value must be a constant (the direct fold) or a plain variable (the
        // inverted fold: `cmpwi; addi r3,r4,1; beqlr; mr r3,c` — verified identical in
        // both orders for one-parameter tails). Computed values stay ordered.
        if hoisted.iter().any(|guard| {
            constant_value(&guard.value).is_none()
                && !matches!(&guard.value, Expression::Variable(_))
        }) {
            return None;
        }
        // The tail (any reassigned value, or the return expression) must not read the
        // parameter living in the result register — the fold clobbers it. Such bodies
        // stay ordered for the branch-form handler.
        if let Some(occupant) = self.locations.iter().find_map(|(name, location)| {
            (location.register == mwcc_target::Eabi::general_result().number
                && location.class == ValueClass::General)
                .then_some(name.as_str())
        }) {
            let tail_reads_occupant = rest.iter().any(|statement| match statement {
                Statement::Assign { value, .. } => expression_reads_name(value, occupant),
                _ => false,
            }) || function
                .return_expression
                .as_ref()
                .is_some_and(|ret| expression_reads_name(ret, occupant));
            if tail_reads_occupant {
                return None;
            }
        }
        // A tail reading TWO OR MORE distinct parameters does not fold directly: mwcc schedules it
        // into the local's home register ahead of the guard value (`add r0,r4,r5; li r3,5; bnelr; mr
        // r3,r0` flat, a real branch ordered) — an order-dependent form that stays ordered for the
        // branch-form handler. Count over the SUBSTITUTED tail so a reassigned parameter read as its
        // tracked value (`c = b + 1; return c;` -> `b + 1`, one parameter) folds like the reassign-
        // in-place shapes, while a self-referential reassignment (`c = b + c` -> `b + c`, two) bails.
        let mut tracked: std::collections::HashMap<String, Expression> =
            std::collections::HashMap::new();
        for local in &function.locals {
            if let Some(initializer) = &local.initializer {
                let value = crate::value_tracking::substitute(initializer, &tracked);
                tracked.insert(local.name.clone(), value);
            }
        }
        for statement in &rest {
            if let Statement::Assign { name, value } = statement {
                let value = crate::value_tracking::substitute(value, &tracked);
                tracked.insert(name.clone(), value);
            }
        }
        if let Some(return_expression) = &function.return_expression {
            let inlined = crate::value_tracking::substitute(return_expression, &tracked);
            let distinct_parameters = function
                .parameters
                .iter()
                .filter(|parameter| expression_reads_name(&inlined, &parameter.name))
                .count();
            // A SELF-REFERENTIAL reassignment (the reassigned name still appears in the substituted
            // tail, `c = b + c` -> `b + c`) reading two-plus parameters keeps its branch form (mwcc
            // computes the tail into the result register AFTER the guard). A NON-self-referential
            // two-parameter tail (`c = b + e` / a fresh local `d = b + c`) instead merges through r0
            // ahead of the guard — that folds via the value-tracking tail-merge, so let it hoist.
            let self_referential = written
                .iter()
                .any(|name| expression_reads_name(&inlined, name));
            if distinct_parameters > 1 && self_referential {
                return None;
            }
        }
        let mut guards = hoisted;
        guards.extend(function.guards.iter().cloned());
        Some(Function {
            statements: rest,
            guards,
            ..function.clone()
        })
    }
}
