//! Live-in parameters that survive a call-valued branch condition.
//!
//! This is a CFG-liveness owner: it promotes the surviving value into a virtual
//! callee-saved home, while the ordinary expression emitter remains responsible
//! for the condition and straight-line arm bodies.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Lower one `if` whose call-valued condition clobbers a parameter needed by
    /// either arm. The initial empty-else form came from Mario Party 4's
    /// `fn_1_0`; the two-arm form occurs in BFBB's `xBaseSave`.
    ///
    /// Keep the allocation boundary intentionally narrow until the oracle
    /// matrix establishes multi-survivor copy and restore schedules: one
    /// general-class live-in and straight-line arm statements.
    pub(crate) fn try_call_condition_live_in_if(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function.return_type != Type::Void
            || function.return_expression.is_some()
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }
        let [Statement::If {
            condition,
            then_body,
            else_body,
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if !expression_has_call(condition)
            || then_body.is_empty()
            || then_body
                .iter()
                .chain(else_body)
                .any(|statement| !straight_line_arm_statement(statement))
        {
            return Ok(false);
        }

        let survivors: Vec<&mwcc_syntax_trees::Parameter> = function
            .parameters
            .iter()
            .filter(|parameter| {
                !expression_reads_name(condition, &parameter.name)
                    && then_body
                        .iter()
                        .chain(else_body)
                        .any(|statement| statement_reads_name(statement, &parameter.name))
            })
            .collect();
        let [survivor] = survivors.as_slice() else {
            return Ok(false);
        };
        let Some(location) = self.locations.get(&survivor.name) else {
            return Ok(false);
        };
        if location.class != ValueClass::General {
            return Ok(false);
        }
        let incoming = location.register;

        self.non_leaf = true;
        let home = self.fresh_virtual_general();
        let plan = mwcc_vreg::FramePlan::sized_for(vec![home]);
        self.frame_size = plan.frame_size;
        self.callee_saved = vec![home];
        self.output
            .instructions
            .extend(plan.prologue_interleaved(&[incoming]));
        if let Some(location) = self.locations.get_mut(&survivor.name) {
            location.register = home;
        }

        let (options, condition_bit) = self.emit_condition_test(condition)?;
        let alternate = self.fresh_label();
        self.emit_branch_conditional_to(options, condition_bit, alternate);
        for statement in then_body {
            self.emit_statement(statement)?;
        }
        if else_body.is_empty() {
            self.bind_label(alternate);
        } else {
            let join = self.fresh_label();
            self.emit_branch_to(join);
            self.bind_label(alternate);
            for statement in else_body {
                self.emit_statement(statement)?;
            }
            self.bind_label(join);
            // Both incoming CFG edges end in calls. mwcc can reload LR before
            // the survivor at their shared join, matching restore-by-death.
            self.epilogue_lr_before_gprs = true;
        }
        self.emit_epilogue_and_return();
        self.output.anonymous_label_bump += 2;
        Ok(true)
    }
}

fn straight_line_arm_statement(statement: &Statement) -> bool {
    matches!(
        statement,
        Statement::Store { .. } | Statement::Assign { .. } | Statement::Expression(_)
    )
}

fn statement_reads_name(statement: &Statement, name: &str) -> bool {
    match statement {
        Statement::Store { target, value } => {
            expression_reads_name(target, name) || expression_reads_name(value, name)
        }
        Statement::Assign { value, .. } | Statement::Expression(value) => {
            expression_reads_name(value, name)
        }
        _ => false,
    }
}
