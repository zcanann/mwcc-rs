//! A computed general value produced between two calls and returned afterward.
//!
//! The first call's result remains live in r3 as the second call's argument;
//! an independent expression is computed without clobbering r3, parked in one
//! callee-saved home, and returned after the second call. A sole parameter used
//! by that expression occupies the same home before its last use, reproducing
//! mwcc's lifetime coalescing (`p` in r31, then the loaded/masked result in r31).

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// `state = enter(); value = EXPR; leave(state); return value;`.
    ///
    /// `EXPR` may read globals/fixed-address memory and, optionally, the sole
    /// general parameter. It may not read `state`, contain a call, or require a
    /// second live parameter. r3 is reserved while evaluating it because it
    /// still contains `state` for `leave`.
    pub(crate) fn try_computed_value_between_calls(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function.locals.len() != 2
            || function.parameters.len() > 1
            || !matches!(
                function.return_type,
                Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. }
            )
            || function.locals.iter().any(|local| {
                local.array_length.is_some() || local.is_static || local.initializer.is_some()
            })
        {
            return Ok(false);
        }
        let [Statement::Assign {
            name: state_name,
            value:
                Expression::Call {
                    name: enter,
                    arguments: enter_arguments,
                },
        }, Statement::Assign {
            name: value_name,
            value,
        }, Statement::Expression(Expression::Call {
            name: leave,
            arguments: leave_arguments,
        })] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        let Some(state) = function
            .locals
            .iter()
            .find(|local| &local.name == state_name)
        else {
            return Ok(false);
        };
        let Some(result) = function
            .locals
            .iter()
            .find(|local| &local.name == value_name)
        else {
            return Ok(false);
        };
        if state.name == result.name
            || !matches!(state.declared_type, Type::Int | Type::UnsignedInt)
            || result.declared_type != function.return_type
            || !enter_arguments.is_empty()
            || !matches!(leave_arguments.as_slice(), [Expression::Variable(name)] if name == &state.name)
            || !matches!(function.return_expression.as_ref(), Some(Expression::Variable(name)) if name == &result.name)
            || expression_has_call(value)
            || expression_reads_name(value, &state.name)
            || matches!(
                self.call_return_types.get(enter.as_str()),
                Some(Type::Float | Type::Double | Type::Void)
            )
        {
            return Ok(false);
        }

        // The optional parameter must be the expression's only register-resident
        // source and must genuinely be read after the entering call.
        let parameter = function.parameters.first();
        if let Some(parameter) = parameter {
            if !matches!(
                parameter.parameter_type,
                Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. }
            ) || !expression_reads_name(value, &parameter.name)
            {
                return Ok(false);
            }
        }
        let incoming = match parameter {
            Some(parameter) => match self.lookup_general(&parameter.name) {
                Some(register) => Some(register),
                None => return Ok(false),
            },
            None => None,
        };

        // One physical callee-saved home serves two non-overlapping source-level
        // values: an optional parameter until EXPR consumes it, then the computed
        // result across `leave`. Reusing one virtual identity expresses that
        // destructive lifetime handoff directly in the transitional vreg stream.
        let home = self.fresh_virtual_general();
        let plan = mwcc_vreg::FramePlan::sized_for(vec![home]);
        self.non_leaf = true;
        self.frame_size = plan.frame_size;
        self.callee_saved = vec![home];
        self.output.instructions.extend(plan.prologue());
        if let (Some(parameter), Some(incoming)) = (parameter, incoming) {
            self.output.instructions.push(Instruction::Or {
                a: home,
                s: incoming,
                b: incoming,
            });
            if let Some(location) = self.locations.get_mut(&parameter.name) {
                location.register = home;
            }
        }

        self.emit_call(enter, enter_arguments, None, false)?;

        // r3 holds `state` until the leave call. Make that ABI liveness visible
        // to the inline placement helpers while the expression is migrated onto
        // the virtual home.
        self.reserved.insert(Eabi::general_result().number);
        let evaluated = self.evaluate_general(value, home);
        self.reserved.remove(&Eabi::general_result().number);
        evaluated?;

        self.locations.insert(
            state.name.clone(),
            Location {
                class: ValueClass::General,
                register: Eabi::general_result().number,
                signed: state.declared_type.is_signed(),
                width: 32,
                pointee: None,
                stride: None,
            },
        );
        self.emit_call(leave, leave_arguments, None, false)?;
        self.output.instructions.push(Instruction::Or {
            a: Eabi::general_result().number,
            s: home,
            b: home,
        });
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
