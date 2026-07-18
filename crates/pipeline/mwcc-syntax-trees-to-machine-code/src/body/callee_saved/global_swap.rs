//! Interrupt-protected global callback swaps.
//!
//! SDK callback registrars share one semantic shape: preserve the incoming
//! callback and the previous global value across disable/restore calls, replace
//! the global while interrupts are disabled, then return the previous value.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// `old = G; enabled = disable(); G = replacement; restore(enabled); return old;`
    ///
    /// Both `old` and `replacement` cross calls, so their virtual live ranges
    /// naturally select r31/r30 from the callee-saved pool. The disable result
    /// remains in r3 through the intervening store and is already in place for
    /// the restore call.
    pub(crate) fn try_interrupt_protected_global_swap(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function.parameters.len() != 1
            || function.locals.len() != 2
        {
            return Ok(false);
        }
        let replacement = &function.parameters[0];
        if function.locals.iter().any(|local| {
            local.array_length.is_some() || local.is_static || local.initializer.is_some()
        }) {
            return Ok(false);
        }
        let [Statement::Assign {
            name: old_name,
            value: Expression::Variable(global_read),
        }, Statement::Assign {
            name: enabled_name,
            value:
                Expression::Call {
                    name: disable,
                    arguments: disable_arguments,
                },
        }, Statement::Store {
            target: Expression::Variable(global_write),
            value: Expression::Variable(replacement_name),
        }, Statement::Expression(Expression::Call {
            name: restore,
            arguments: restore_arguments,
        })] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        // Declaration order is immaterial: AI declares the interrupt state
        // before the old callback, while AR declares them in use order.
        let Some(old) = function.locals.iter().find(|local| &local.name == old_name) else {
            return Ok(false);
        };
        let Some(enabled) = function
            .locals
            .iter()
            .find(|local| &local.name == enabled_name)
        else {
            return Ok(false);
        };
        if old.name == enabled.name
            || old.declared_type != replacement.parameter_type
            || function.return_type != old.declared_type
            || !matches!(enabled.declared_type, Type::Int | Type::UnsignedInt)
            || global_read != global_write
            || replacement_name != &replacement.name
            || !disable_arguments.is_empty()
            || !matches!(restore_arguments.as_slice(), [Expression::Variable(name)] if name == &enabled.name)
            || !matches!(function.return_expression.as_ref(), Some(Expression::Variable(name)) if name == &old.name)
            || self.globals.get(global_read.as_str()) != Some(&old.declared_type)
            || matches!(
                self.call_return_types.get(disable.as_str()),
                Some(Type::Float | Type::Double | Type::Void)
            )
        {
            return Ok(false);
        }

        let Some(incoming) = self.lookup_general(&replacement.name) else {
            return Ok(false);
        };
        let old_home = self.fresh_virtual_general();
        let replacement_home = self.fresh_virtual_general();
        let homes = vec![old_home, replacement_home];
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        self.non_leaf = true;
        self.frame_size = plan.frame_size;
        self.callee_saved = homes;
        self.output.instructions.extend(plan.prologue());

        self.output.instructions.push(Instruction::Or {
            a: replacement_home,
            s: incoming,
            b: incoming,
        });
        self.emit_global_load(global_read, old_home)?;
        self.emit_call(disable, disable_arguments, None, false)?;
        self.emit_global_store(global_write, Pointee::UnsignedInt, replacement_home)?;

        // The disable result is the enabled local, still resident in r3. Register
        // it so the shared argument emitter proves the restore argument is an
        // in-place passthrough rather than materializing an unnecessary move.
        self.locations.insert(
            enabled.name.clone(),
            Location {
                class: ValueClass::General,
                register: Eabi::general_result().number,
                signed: enabled.declared_type.is_signed(),
                width: 32,
                pointee: None,
                stride: None,
            },
        );
        self.emit_call(restore, restore_arguments, None, false)?;
        self.output.instructions.push(Instruction::Or {
            a: Eabi::general_result().number,
            s: old_home,
            b: old_home,
        });
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
