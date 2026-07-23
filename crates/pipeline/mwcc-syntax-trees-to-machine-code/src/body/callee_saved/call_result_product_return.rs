//! A local scalar call multiplied by a second live-in parameter.
//!
//! Without whole-file IPA the multiplier crosses the call: optimized builds
//! park it in r31, while O0 homes both incoming parameters in the linkage
//! frame. IPA substitutes the local callee before this owner and leaves the
//! ordinary leaf-expression scheduler in charge.

#[allow(unused_imports)]
use super::*;
use mwcc_versions::Optimization;

impl Generator {
    pub(crate) fn try_call_result_product_return(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.whole_file_optimization
            || self.behavior.frame_convention != FrameConvention::Predecrement
            || !self.frame_slots.is_empty()
            || !function.locals.is_empty()
            || !function.statements.is_empty()
            || !function.guards.is_empty()
            || !matches!(function.return_type, Type::Int | Type::UnsignedInt)
        {
            return Ok(false);
        }
        let [call_parameter, live_parameter] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if !matches!(
            (call_parameter.parameter_type, live_parameter.parameter_type),
            (Type::Int | Type::UnsignedInt, Type::Int | Type::UnsignedInt)
        ) || self
            .locations
            .get(&call_parameter.name)
            .map(|location| (location.class, location.register))
            != Some((ValueClass::General, Eabi::FIRST_GENERAL_ARGUMENT))
            || self
                .locations
                .get(&live_parameter.name)
                .map(|location| (location.class, location.register))
                != Some((ValueClass::General, Eabi::FIRST_GENERAL_ARGUMENT + 1))
        {
            return Ok(false);
        }
        let Some((callee, constant)) = call_product_shape(
            function.return_expression.as_ref(),
            &call_parameter.name,
            &live_parameter.name,
        ) else {
            return Ok(false);
        };
        if self.locations.contains_key(callee) || self.globals.contains_key(callee) {
            return Ok(false);
        }

        match self.behavior.optimization {
            Optimization::O0 => self.emit_unoptimized_call_product(callee, constant),
            Optimization::O2 | Optimization::O3 | Optimization::O4 => {
                self.emit_optimized_call_product(callee, constant)
            }
            Optimization::O1 => return Ok(false),
        }
        Ok(true)
    }

    fn emit_optimized_call_product(&mut self, callee: &str, constant: i16) {
        self.non_leaf = true;
        self.frame_size = 16;
        let saved = self.fresh_virtual_general();
        self.callee_saved = vec![saved];
        self.output
            .instructions
            .extend(mwcc_vreg::FramePlan::sized_for(vec![saved]).prologue());
        self.output.instructions.push(Instruction::move_register(
            saved,
            Eabi::FIRST_GENERAL_ARGUMENT + 1,
        ));
        self.record_relocation(RelocationKind::Rel24, callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: callee.to_owned(),
        });
        self.output.instructions.push(Instruction::MultiplyLow {
            d: Eabi::general_result().number,
            a: saved,
            b: Eabi::general_result().number,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: Eabi::general_result().number,
            a: Eabi::general_result().number,
            immediate: constant,
        });
        self.emit_epilogue_and_return();
    }

    fn emit_unoptimized_call_product(&mut self, callee: &str, constant: i16) {
        self.non_leaf = true;
        self.frame_size = 16;
        self.output.instructions.extend([
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 20,
            },
            Instruction::StoreWord {
                s: Eabi::FIRST_GENERAL_ARGUMENT,
                a: 1,
                offset: 8,
            },
            Instruction::StoreWord {
                s: Eabi::FIRST_GENERAL_ARGUMENT + 1,
                a: 1,
                offset: 12,
            },
            Instruction::LoadWord {
                d: Eabi::FIRST_GENERAL_ARGUMENT,
                a: 1,
                offset: 8,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: callee.to_owned(),
        });
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 12,
            },
            Instruction::MultiplyLow {
                d: Eabi::general_result().number,
                a: 0,
                b: Eabi::general_result().number,
            },
            Instruction::AddImmediate {
                d: Eabi::general_result().number,
                a: Eabi::general_result().number,
                immediate: constant,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 20,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 16,
            },
            Instruction::BranchToLinkRegister,
        ]);
    }
}

fn call_product_shape<'a>(
    expression: Option<&'a Expression>,
    call_parameter: &str,
    live_parameter: &str,
) -> Option<(&'a str, i16)> {
    let Expression::Binary {
        operator: BinaryOperator::Add,
        left,
        right,
    } = expression?
    else {
        return None;
    };
    let (product, constant) = match (left.as_ref(), right.as_ref()) {
        (product, Expression::IntegerLiteral(constant)) => (product, *constant),
        (Expression::IntegerLiteral(constant), product) => (product, *constant),
        _ => return None,
    };
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left,
        right,
    } = product
    else {
        return None;
    };
    let call = match (left.as_ref(), right.as_ref()) {
        (Expression::Variable(name), call) | (call, Expression::Variable(name))
            if name == live_parameter => call,
        _ => return None,
    };
    let Expression::Call { name, arguments } = call else {
        return None;
    };
    if !matches!(arguments.as_slice(), [Expression::Variable(argument)] if argument == call_parameter)
    {
        return None;
    }
    Some((name.as_str(), i16::try_from(constant).ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_commuted_product_and_addend() {
        let expression = Expression::Binary {
            operator: BinaryOperator::Add,
            left: Box::new(Expression::IntegerLiteral(54)),
            right: Box::new(Expression::Binary {
                operator: BinaryOperator::Multiply,
                left: Box::new(Expression::Call {
                    name: "align".into(),
                    arguments: vec![Expression::Variable("width".into())],
                }),
                right: Box::new(Expression::Variable("height".into())),
            }),
        };
        assert_eq!(
            call_product_shape(Some(&expression), "width", "height"),
            Some(("align", 54))
        );
    }
}
