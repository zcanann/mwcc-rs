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
    /// Entry copies happen before the condition, but condition arguments retain
    /// their incoming homes through the call. The selected arm then switches to
    /// the saved homes. This boundary is essential when the same object pointer
    /// feeds both the call condition and a later arm call.
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

        let survivors = arm_live_parameters(&function.parameters, then_body, else_body);
        if survivors.is_empty() {
            return Ok(false);
        }
        let Some(incoming) = survivors
            .iter()
            .map(|survivor| {
                self.locations
                    .get(&survivor.name)
                    .filter(|location| location.class == ValueClass::General)
                    .map(|location| location.register)
            })
            .collect::<Option<Vec<_>>>()
        else {
            return Ok(false);
        };

        self.non_leaf = true;
        // Inline accessors expanded into this branch have no temporary live
        // across the condition call.  The general inline-residue estimate is
        // intentionally conservative for straight-line bodies; this CFG owner
        // has enough liveness information to discharge that retained lane.
        self.legacy_inline_expansion_frame_bytes = 0;
        let homes: Vec<u8> = survivors
            .iter()
            .map(|_| self.fresh_virtual_general())
            .collect();
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        self.frame_size = plan.frame_size;
        self.callee_saved = homes.clone();
        self.output
            .instructions
            .extend(plan.prologue_interleaved(&incoming));

        // The condition still sees every parameter in its incoming EABI home;
        // only the arm crosses the call and consumes the saved copies.
        let (options, condition_bit) = self.emit_condition_test(condition)?;
        for (survivor, home) in survivors.iter().zip(&homes) {
            if let Some(location) = self.locations.get_mut(&survivor.name) {
                location.register = *home;
            }
        }
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

    /// Finish the build-163 issue order for a call in the selected arm.  When
    /// two saved entry values are forwarded and one also feeds a computed
    /// earlier argument, MWCC issues the independent forwards first and spells
    /// the whole copy group as `addi ...,0`.
    pub(crate) fn schedule_call_condition_live_in_arguments(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }
        schedule_forwarded_argument_group(&mut self.output.instructions);
    }
}

fn schedule_forwarded_argument_group(instructions: &mut [Instruction]) -> bool {
    if instructions.len() < 4 {
        return false;
    }
    for index in 0..=instructions.len() - 4 {
        let (computed_base, computed_offset, first_source, second_source) =
            match &instructions[index..index + 4] {
                [
                    Instruction::AddImmediate {
                        d: 4,
                        a: computed_base,
                        immediate: computed_offset,
                    },
                    Instruction::Or { a: 5, s: first_source, b: first_source_b },
                    Instruction::Or { a: 6, s: second_source, b: second_source_b },
                    Instruction::BranchAndLink { .. },
                ] if first_source == first_source_b
                    && second_source == second_source_b
                    && computed_base == first_source
                    && *computed_offset != 0
                    // This is a post-allocation pass. Registers r14-r31 are the
                    // physical nonvolatile GPR set; the generator's saved-home
                    // list still contains its virtual identities here.
                    && *first_source >= 14
                    && *second_source >= 14 =>
                {
                    (*computed_base, *computed_offset, *first_source, *second_source)
                }
                _ => continue,
            };
        instructions[index] = Instruction::AddImmediate {
            d: 5,
            a: first_source,
            immediate: 0,
        };
        instructions[index + 1] = Instruction::AddImmediate {
            d: 6,
            a: second_source,
            immediate: 0,
        };
        instructions[index + 2] = Instruction::AddImmediate {
            d: 4,
            a: computed_base,
            immediate: computed_offset,
        };
        return true;
    }
    false
}

fn straight_line_arm_statement(statement: &Statement) -> bool {
    matches!(
        statement,
        Statement::Store { .. } | Statement::Assign { .. } | Statement::Expression(_)
    )
}

fn arm_live_parameters<'a>(
    parameters: &'a [mwcc_syntax_trees::Parameter],
    then_body: &[Statement],
    else_body: &[Statement],
) -> Vec<&'a mwcc_syntax_trees::Parameter> {
    parameters
        .iter()
        .rev()
        .filter(|parameter| {
            then_body
                .iter()
                .chain(else_body)
                .any(|statement| statement_reads_name(statement, &parameter.name))
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retains_parameters_read_by_both_the_call_condition_and_its_arm() {
        let parameters = vec![
            mwcc_syntax_trees::Parameter {
                parameter_type: Type::StructPointer { element_size: 8 },
                name: "object".into(),
            },
            mwcc_syntax_trees::Parameter {
                parameter_type: Type::Int,
                name: "amount".into(),
            },
        ];
        let arm = vec![Statement::Expression(Expression::Call {
            name: "consume".into(),
            arguments: vec![
                Expression::Variable("object".into()),
                Expression::Variable("amount".into()),
            ],
        })];

        let names: Vec<_> = arm_live_parameters(&parameters, &arm, &[])
            .into_iter()
            .map(|parameter| parameter.name.as_str())
            .collect();
        assert_eq!(names, ["amount", "object"]);
    }

    #[test]
    fn forwards_saved_arguments_before_their_computed_sibling() {
        let mut instructions = vec![
            Instruction::AddImmediate { d: 4, a: 30, immediate: 2 },
            Instruction::Or { a: 5, s: 30, b: 30 },
            Instruction::Or { a: 6, s: 31, b: 31 },
            Instruction::BranchAndLink { target: "sink".into() },
        ];

        assert!(schedule_forwarded_argument_group(&mut instructions));
        assert_eq!(
            instructions,
            vec![
                Instruction::AddImmediate { d: 5, a: 30, immediate: 0 },
                Instruction::AddImmediate { d: 6, a: 31, immediate: 0 },
                Instruction::AddImmediate { d: 4, a: 30, immediate: 2 },
                Instruction::BranchAndLink { target: "sink".into() },
            ]
        );
    }
}
