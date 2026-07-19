//! Whole-file IPA sibling-call elimination.

use super::*;

impl Generator {
    /// Lower a trivial terminal direct call as `b target` under `-ipa file`.
    ///
    /// The arguments are still marshaled through the ordinary call path; only
    /// the link-producing call and now-dead epilogue are replaced. Restricting
    /// this to a body with no locals, guards, or preceding statements guarantees
    /// there is no live frame state to restore before transferring control.
    pub(crate) fn try_tail_call(&mut self, function: &Function) -> Compilation<bool> {
        if !self.behavior.tail_call_optimization
            || !function.locals.is_empty()
            || !function.guards.is_empty()
            || !function.statements.is_empty()
            || !self.frame_slots.is_empty()
            || self.variadic_definition
        {
            return Ok(false);
        }
        let Some(Expression::Call { name, arguments }) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        if self.locations.contains_key(name)
            || self.globals.contains_key(name)
            || self.variadic_callees.contains(name)
            || self.call_return_types.get(name) != Some(&function.return_type)
        {
            return Ok(false);
        }

        self.emit_arguments(arguments, name)?;
        self.record_relocation(RelocationKind::Rel24, name);
        self.output.instructions.push(Instruction::BranchExternal {
            target: name.clone(),
        });
        Ok(true)
    }
}
