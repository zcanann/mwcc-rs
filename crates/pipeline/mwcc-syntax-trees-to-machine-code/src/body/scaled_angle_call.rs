//! Narrow angle-code call converted to a scaled single-precision result.
//!
//! MWCC schedules the swapped float arguments through the linkage-first
//! prologue, then converts the unsigned-short result with the 2^52 bias
//! technique before applying a literal scale. The call, conversion frame, and
//! float DAG are one scheduling region.

use super::*;
use mwcc_machine_code::RelocationTarget;

struct ScaledAngleCall<'a> {
    callee: &'a str,
    first_argument: &'a str,
    second_argument: &'a str,
    divisor: f32,
    multiplier: f32,
}

fn classify<'a>(
    function: &'a Function,
    call_return_types: &std::collections::HashMap<String, Type>,
    call_parameter_types: &std::collections::HashMap<String, Vec<Type>>,
) -> Option<ScaledAngleCall<'a>> {
    if function.return_type != Type::Float
        || !function.locals.is_empty()
        || !function.statements.is_empty()
        || !function.guards.is_empty()
        || function.asm_body.is_some()
    {
        return None;
    }
    let [first, second] = function.parameters.as_slice() else {
        return None;
    };
    if first.parameter_type != Type::Float || second.parameter_type != Type::Float {
        return None;
    }
    let Expression::Binary {
        operator: BinaryOperator::Multiply,
        left: divided,
        right: multiplier,
    } = function.return_expression.as_ref()?
    else {
        return None;
    };
    let Expression::FloatLiteral(multiplier) = rightmost_value(multiplier) else {
        return None;
    };
    let Expression::Binary {
        operator: BinaryOperator::Divide,
        left: called,
        right: divisor,
    } = divided.as_ref()
    else {
        return None;
    };
    let Expression::FloatLiteral(divisor) = divisor.as_ref() else {
        return None;
    };
    let Expression::Call {
        name: callee,
        arguments,
    } = called.as_ref()
    else {
        return None;
    };
    let [
        Expression::Variable(first_argument),
        Expression::Variable(second_argument),
    ] = arguments.as_slice()
    else {
        return None;
    };
    if call_return_types.get(callee) != Some(&Type::UnsignedShort)
        || call_parameter_types.get(callee).map(Vec::as_slice)
            != Some(&[Type::Float, Type::Float])
        || !divisor.is_finite()
        || *divisor <= 0.0
        || !multiplier.is_finite()
        || first_argument != &second.name
        || second_argument != &first.name
    {
        return None;
    }
    Some(ScaledAngleCall {
        callee,
        first_argument,
        second_argument,
        divisor: *divisor as f32,
        multiplier: *multiplier as f32,
    })
}

/// Const-global substitution has already rewritten the multiplier to a literal
/// before body lowering. Keep a tiny wrapper so a harmless cast introduced by
/// later normalization does not obscure that leaf.
fn rightmost_value(expression: &Expression) -> &Expression {
    match expression {
        Expression::Cast { operand, .. } => rightmost_value(operand),
        _ => expression,
    }
}

impl Generator {
    pub(crate) fn try_scaled_angle_call(&mut self, function: &Function) -> Compilation<bool> {
        let Some(shape) = classify(
            function,
            &self.call_return_types,
            &self.call_parameter_types,
        ) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.float_register_of(shape.first_argument)? != 2
            || self.float_register_of(shape.second_argument)? != 1
            || !self.frame_slots.is_empty()
        {
            return Ok(false);
        }

        // Reverse expression visitation assigns the multiplier and divisor
        // first; the conversion's double bias is created after its internal
        // label. Emit in scheduled order below, retaining those pool identities.
        let multiplier = self
            .output
            .intern_constant(shape.multiplier.to_bits() as u64, 4);
        let divisor = self
            .output
            .intern_constant(shape.divisor.to_bits() as u64, 4);
        let conversion_bias = self
            .output
            .intern_constant(0x4330_0000_0000_0000, 8);
        self.output.constant_number_gaps.push((conversion_bias, 1));

        self.non_leaf = true;
        self.frame_size = 24;
        self.output.pre_scheduled = true;
        self.output.symbol_order = vec![shape.callee.to_owned()];
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 0, b: 2 });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output
            .instructions
            .push(Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            });
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 2, b: 1 });
        self.output
            .instructions
            .push(Instruction::FloatMove { d: 1, b: 0 });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -24,
            });
        self.record_relocation(RelocationKind::Rel24, shape.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.callee.to_owned(),
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: 3,
                clear: 16,
            });
        self.record_target(
            RelocationKind::EmbSda21,
            RelocationTarget::Constant(conversion_bias),
        );
        self.output
            .instructions
            .push(Instruction::LoadFloatDouble {
                d: 3,
                a: 0,
                offset: 0,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 0x4330));
        self.record_target(RelocationKind::EmbSda21, RelocationTarget::Constant(divisor));
        self.output
            .instructions
            .push(Instruction::LoadFloatSingle {
                d: 1,
                a: 0,
                offset: 0,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 16,
        });
        self.record_target(
            RelocationKind::EmbSda21,
            RelocationTarget::Constant(multiplier),
        );
        self.output
            .instructions
            .push(Instruction::LoadFloatSingle {
                d: 0,
                a: 0,
                offset: 0,
            });
        self.output
            .instructions
            .push(Instruction::LoadFloatDouble {
                d: 2,
                a: 1,
                offset: 16,
            });
        self.output
            .instructions
            .push(Instruction::FloatSubtractSingle { d: 2, a: 2, b: 3 });
        self.output
            .instructions
            .push(Instruction::FloatDivideSingle { d: 1, a: 2, b: 1 });
        self.output
            .instructions
            .push(Instruction::FloatMultiplySingle { d: 1, a: 0, c: 1 });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 28,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 24,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
