//! Source control-flow scope for numeric-conversion stack images.
//!
//! MWCC gives conversions in one straight-line basic block distinct temporary
//! images so their schedules can be pipelined. A conversion in a branch, and
//! one in the continuation after that branch, reuse the same low frame lane.
//! This module proves the conservative one-lane case. More general block-local
//! lane numbering can extend this analysis without teaching frame layout about
//! individual expression shapes.

use crate::generator::Generator;
use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::{Expression, Function, Statement};

#[derive(Default)]
struct BlockUsage {
    current: usize,
    maximum: usize,
    total: usize,
}

impl BlockUsage {
    fn add(&mut self, count: usize) {
        self.current = self.current.saturating_add(count);
        self.total = self.total.saturating_add(count);
        self.maximum = self.maximum.max(self.current);
    }

    fn boundary(&mut self) {
        self.current = 0;
    }
}

impl Generator {
    /// Prove that every source basic block owns at most one stack-backed
    /// numeric conversion. The total from the isolated block walk must equal
    /// the established whole-function counters; if a new AST form is not
    /// represented here, the mismatch conservatively keeps disjoint images.
    ///
    /// Real MWCC keeps distinct images for smaller leaf shapes even when that
    /// block property holds. Until the complete allocator heuristic is known,
    /// require the observed non-leaf, four-or-more-conversion shape as well.
    pub(crate) fn basic_blocks_use_one_numeric_conversion_lane(
        &self,
        function: &Function,
        int_to_float_count: usize,
        float_to_int_count: usize,
    ) -> bool {
        let expected = int_to_float_count.saturating_add(float_to_int_count);
        if int_to_float_count == 0
            || float_to_int_count == 0
            || expected < 4
            || !function.guards.is_empty()
            || !crate::analysis::function_makes_call(function)
        {
            return false;
        }

        let mut usage = BlockUsage::default();
        for index in 0..function.locals.len() {
            if function.locals[index].initializer.is_none() {
                continue;
            }
            let mut isolated = stripped_function(function);
            isolated.locals[index].initializer = function.locals[index].initializer.clone();
            usage.add(self.numeric_conversion_count(&isolated));
        }
        if !self.walk_numeric_conversion_blocks(function, &function.statements, &mut usage) {
            return false;
        }
        if let Some(return_expression) = &function.return_expression {
            let mut isolated = stripped_function(function);
            isolated.return_expression = Some(return_expression.clone());
            usage.add(self.numeric_conversion_count(&isolated));
        }

        if std::env::var_os("MWCC_DIAGNOSTIC_CONVERSION_SCOPE").is_some() {
            eprintln!(
                "conversion scope {}: expected={expected} observed={} maximum={} guards={}",
                function.name,
                usage.total,
                usage.maximum,
                function.guards.len()
            );
        }
        usage.total == expected && usage.maximum == 1
    }

    /// Configure the single image proved above. Both direction-specific
    /// cursors are marked consumed because later frame reconciliation uses
    /// `next == end` as its completeness invariant; claims themselves return
    /// the shared base directly.
    pub(crate) fn plan_shared_numeric_conversion_scratch(
        &mut self,
        base: i16,
        has_int_to_float: bool,
        has_float_to_int: bool,
    ) -> Compilation<()> {
        let end = base
            .checked_add(8)
            .ok_or_else(|| Diagnostic::error("numeric-conversion scratch range is too large"))?;
        self.shared_numeric_conversion_scratch = Some(base);
        if has_int_to_float {
            self.int_to_float_scratch_next = end;
            self.int_to_float_scratch_end = end;
        }
        if has_float_to_int {
            self.float_to_int_scratch_next = end;
            self.float_to_int_scratch_end = end;
        }
        Ok(())
    }

    fn walk_numeric_conversion_blocks(
        &self,
        function: &Function,
        statements: &[Statement],
        usage: &mut BlockUsage,
    ) -> bool {
        for statement in statements {
            match statement {
                Statement::Store { .. }
                | Statement::Assign { .. }
                | Statement::Expression(_) => {
                    usage.add(self.isolated_statement_conversion_count(function, statement));
                }
                Statement::If {
                    condition,
                    then_body,
                    else_body,
                } => {
                    usage.add(self.isolated_expression_conversion_count(function, condition));
                    usage.boundary();
                    if !self.walk_numeric_conversion_blocks(function, then_body, usage) {
                        return false;
                    }
                    usage.boundary();
                    if !self.walk_numeric_conversion_blocks(function, else_body, usage) {
                        return false;
                    }
                    usage.boundary();
                }
                Statement::Return(_) => {
                    usage.add(self.isolated_statement_conversion_count(function, statement));
                    usage.boundary();
                }
                // Loops, switches, and unstructured edges need block-specific
                // recurrence/fallthrough accounting before they can opt in.
                Statement::InlineAsm(_)
                | Statement::Switch { .. }
                | Statement::Loop { .. }
                | Statement::Break
                | Statement::Continue
                | Statement::Goto(_)
                | Statement::Label(_) => return false,
            }
        }
        true
    }

    fn isolated_statement_conversion_count(
        &self,
        function: &Function,
        statement: &Statement,
    ) -> usize {
        let mut isolated = stripped_function(function);
        isolated.statements.push(statement.clone());
        self.numeric_conversion_count(&isolated)
    }

    fn isolated_expression_conversion_count(
        &self,
        function: &Function,
        expression: &Expression,
    ) -> usize {
        let mut isolated = stripped_function(function);
        isolated
            .statements
            .push(Statement::Expression(expression.clone()));
        self.numeric_conversion_count(&isolated)
    }

    fn numeric_conversion_count(&self, function: &Function) -> usize {
        self.count_integer_to_float_conversions(function)
            .saturating_add(self.count_float_to_integer_conversions(function))
    }
}

fn stripped_function(function: &Function) -> Function {
    let mut isolated = function.clone();
    for local in &mut isolated.locals {
        local.initializer = None;
    }
    isolated.statements.clear();
    isolated.guards.clear();
    isolated.return_expression = None;
    isolated
}
