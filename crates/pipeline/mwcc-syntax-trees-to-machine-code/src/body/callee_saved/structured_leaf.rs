//! Frame-free entry point for the shared structured-control-flow lowerer.
//!
//! Structured lowering originally lived behind the callee-saved frame owner,
//! even though its branch graph is equally useful for leaf functions. This
//! adapter owns no prologue or allocation policy: it admits only frame-free
//! bodies and delegates their nested regions to the common emitter.

#[allow(unused_imports)]
use super::*;
use super::structured::structured_hidden_label_count;

impl Generator {
    pub(crate) fn try_leaf_structured_body(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function_makes_call(function)
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || !self.frame_slots.is_empty()
            || function.return_type != Type::Void
            || function.return_expression.is_some()
            || !contains_nested_or_else_if(&function.statements)
            || !supports_leaf_structured_statements(&function.statements)
        {
            return Ok(false);
        }

        let mut return_branches = Vec::new();
        let mut label_positions = std::collections::HashMap::new();
        let mut pending_gotos = Vec::new();
        self.emit_structured_statements(
            &function.statements,
            function,
            &[],
            false,
            &mut return_branches,
            &mut label_positions,
            &mut pending_gotos,
            &mut None,
        )?;
        debug_assert!(return_branches.is_empty());
        debug_assert!(pending_gotos.is_empty());
        self.output.anonymous_label_bump += structured_hidden_label_count(&function.statements);
        self.emit_epilogue_and_return();
        Ok(true)
    }
}

fn contains_nested_or_else_if(statements: &[Statement]) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            !else_body.is_empty()
                || then_body.iter().any(|inner| matches!(inner, Statement::If { .. }))
                || contains_nested_or_else_if(then_body)
                || contains_nested_or_else_if(else_body)
        }
        _ => false,
    })
}

fn supports_leaf_structured_statements(statements: &[Statement]) -> bool {
    statements.iter().all(|statement| match statement {
        Statement::Assign { .. } | Statement::Store { .. } => true,
        Statement::If {
            then_body,
            else_body,
            ..
        } => {
            supports_leaf_structured_statements(then_body)
                && supports_leaf_structured_statements(else_body)
        }
        _ => false,
    })
}
