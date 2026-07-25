//! Incoming parameters retained through computed declaration initializers.
//!
//! A parameter can feed an inlined initializer and still be needed by the
//! first body call. It does not cross a call, so a nonvolatile home would
//! overstate its lifetime. MWCC instead keeps it in the first volatile GPR
//! beyond that call's argument window.

use super::structured_locals::body_uses_local;
#[allow(unused_imports)]
use super::*;

pub(super) struct InitializerLiveIn {
    pub(super) name: String,
    pub(super) incoming: u8,
    pub(super) preferred: u8,
}

impl Generator {
    pub(super) fn plan_initializer_live_in(
        &self,
        function: &Function,
        eager_saved_locals: &[&LocalDeclaration],
        saved_parameter_names: &std::collections::HashSet<&str>,
    ) -> Option<InitializerLiveIn> {
        let Statement::Expression(Expression::Call { arguments, .. }) =
            function.statements.first()?
        else {
            return None;
        };
        if arguments
            .iter()
            .any(|argument| self.is_float_value(argument))
        {
            return None;
        }
        let preferred = Eabi::FIRST_GENERAL_ARGUMENT
            .checked_add(u8::try_from(arguments.len()).ok()?)
            .filter(|register| *register <= Eabi::LAST_GENERAL_ARGUMENT)?;

        let mut candidates = function.parameters.iter().filter_map(|parameter| {
            if saved_parameter_names.contains(parameter.name.as_str())
                || !body_uses_local(&function.statements, &parameter.name)
                || !arguments
                    .iter()
                    .any(|argument| expression_reads_name(argument, &parameter.name))
                || !eager_saved_locals.iter().any(|local| {
                    local.initializer.as_ref().is_some_and(|initializer| {
                        expression_reads_name(initializer, &parameter.name)
                            && !matches!(
                                initializer,
                                Expression::Variable(_) | Expression::IntegerLiteral(_)
                            )
                    })
                })
            {
                return None;
            }
            let location = self.locations.get(&parameter.name)?;
            (location.class == ValueClass::General
                && location.width == 32
                && (Eabi::FIRST_GENERAL_ARGUMENT..=Eabi::LAST_GENERAL_ARGUMENT)
                    .contains(&location.register))
            .then(|| InitializerLiveIn {
                name: parameter.name.clone(),
                incoming: location.register,
                preferred,
            })
        });
        let plan = candidates.next()?;
        candidates.next().is_none().then_some(plan)
    }

    pub(super) fn emit_initializer_live_in(&mut self, plan: InitializerLiveIn) {
        let retained = self.fresh_virtual_general_preferring(plan.preferred);
        self.output
            .instructions
            .push(Instruction::move_register(retained, plan.incoming));
        self.locations
            .get_mut(&plan.name)
            .expect("initializer live-in parameter was planned")
            .register = retained;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_use_detection_sees_a_parameter_nested_in_a_call_argument() {
        let statements = vec![Statement::Expression(Expression::Call {
            name: "copy".into(),
            arguments: vec![Expression::Binary {
                operator: BinaryOperator::Add,
                left: Box::new(Expression::Variable("base".into())),
                right: Box::new(Expression::Variable("offset".into())),
            }],
        })];
        assert!(body_uses_local(&statements, "offset"));
    }
}
