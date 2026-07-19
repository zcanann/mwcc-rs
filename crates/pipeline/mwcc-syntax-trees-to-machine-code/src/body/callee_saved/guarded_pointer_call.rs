//! Guarded calls through global function pointers captured in local aliases.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// A guarded call through a global function pointer held in a local (the signal.c
    /// dispatch tail): `F t = gf; if (!t) return; t();`. Predecrement builds keep the
    /// pointer in r12 and call through CTR. Build 163 stages the load in r0, copies it
    /// into r12 after the comparison, and calls through LR.
    pub(crate) fn try_guarded_global_pointer_call(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
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
        if !self.globals.contains_key(global.as_str())
            || self.global_array_sizes.contains_key(global.as_str())
        {
            return Ok(false);
        }
        let [Statement::If {
            condition,
            then_body,
            else_body,
        }, Statement::Expression(Expression::Call { name, arguments })] =
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

        // Arguments must already occupy their ABI registers. Nothing is materialized,
        // so an argument-preserving form has the same pointer schedule as `t()`.
        for (position, argument) in arguments.iter().enumerate() {
            let Expression::Variable(argument_name) = argument else {
                return Ok(false);
            };
            let expected = mwcc_target::Eabi::FIRST_GENERAL_ARGUMENT + position as u8;
            match self.locations.get(argument_name) {
                Some(location)
                    if location.class == ValueClass::General && location.register == expected => {}
                _ => return Ok(false),
            }
        }

        let plan = mwcc_vreg::FramePlan::sized_for(Vec::new());
        self.non_leaf = true;
        self.frame_size = plan.frame_size;
        self.output.instructions.extend(plan.prologue());

        let staged = match self.behavior.frame_convention {
            FrameConvention::Predecrement => 12,
            FrameConvention::LinkageFirst => GENERAL_SCRATCH,
        };
        self.emit_global_load_value(global, staged)?;
        self.locations.insert(
            local.name.clone(),
            Location {
                class: ValueClass::General,
                register: staged,
                signed: false,
                width: 32,
                pointee: None,
                stride: None,
            },
        );
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        if staged != 12 {
            self.output.instructions.push(Instruction::Or {
                a: 12,
                s: staged,
                b: staged,
            });
        }

        self.output.anonymous_label_bump = 3;
        let epilogue_branch = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: options ^ 8,
                condition_bit,
                target: 0,
            });
        self.emit_indirect_branch_and_link(12);
        let epilogue_label = self.output.instructions.len();
        if let Instruction::BranchConditionalForward { target, .. } =
            &mut self.output.instructions[epilogue_branch]
        {
            *target = epilogue_label;
        }
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
