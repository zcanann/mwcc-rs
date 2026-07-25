//! One-call initialization split across a large state aggregate and a scalar.
//!
//! MWCC schedules the first aggregate base through the prologue, then batches
//! the post-call aggregate and scalar bases before publishing either result.
//! Keeping the complete transaction here makes base lifetime and register
//! assignment explicit instead of rematerializing each store independently.

#[allow(unused_imports)]
use super::*;

struct GlobalStateInitialization<'a> {
    aggregate: &'a str,
    first_offset: i16,
    first_value: i16,
    call_offset: i16,
    callee: &'a str,
    scalar: &'a str,
    scalar_high: i16,
    return_value: i16,
}

impl Generator {
    pub(crate) fn try_global_state_initialization(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = self.global_state_initialization(function) else {
            return Ok(false);
        };
        self.emit_global_state_initialization(&plan);
        Ok(true)
    }

    fn global_state_initialization<'a>(
        &self,
        function: &'a Function,
    ) -> Option<GlobalStateInitialization<'a>> {
        if self.behavior.frame_convention != FrameConvention::Predecrement
            || self.behavior.global_addressing != GlobalAddressing::Absolute
            || !self.frame_slots.is_empty()
            || !function.parameters.is_empty()
            || !function.locals.is_empty()
            || !function.guards.is_empty()
            || !matches!(function.return_type, Type::Int | Type::UnsignedInt)
        {
            return None;
        }
        let [first, called, scalar] = function.statements.as_slice() else {
            return None;
        };
        let (
            Statement::Store {
                target:
                    Expression::Member {
                        base: first_base,
                        offset: first_offset,
                        member_type: first_type,
                        index_stride: None,
                    },
                value: first_value,
            },
            Statement::Store {
                target:
                    Expression::Member {
                        base: call_base,
                        offset: call_offset,
                        member_type: call_type,
                        index_stride: None,
                    },
                value:
                    Expression::Call {
                        name: callee,
                        arguments,
                    },
            },
            Statement::Store {
                target: Expression::Variable(scalar),
                value: scalar_value,
            },
        ) = (first, called, scalar)
        else {
            return None;
        };
        let (Expression::Variable(aggregate), Expression::Variable(call_aggregate)) =
            (first_base.as_ref(), call_base.as_ref())
        else {
            return None;
        };
        let scalar_value = u32::try_from(constant_value(scalar_value)?).ok()?;
        if aggregate != call_aggregate
            || !arguments.is_empty()
            || !matches!(first_type, Type::Int | Type::UnsignedInt)
            || !matches!(call_type, Type::Int | Type::UnsignedInt)
            || !matches!(self.globals.get(aggregate), Some(Type::Struct { size, .. }) if *size > 8)
            || !matches!(
                self.globals.get(scalar),
                Some(Type::Int | Type::UnsignedInt)
            )
            || scalar_value & 0xffff != 0
        {
            return None;
        }
        Some(GlobalStateInitialization {
            aggregate,
            first_offset: i16::try_from(*first_offset).ok()?,
            first_value: i16::try_from(constant_value(first_value)?).ok()?,
            call_offset: i16::try_from(*call_offset).ok()?,
            callee,
            scalar,
            scalar_high: (scalar_value >> 16) as u16 as i16,
            return_value: i16::try_from(constant_value(function.return_expression.as_ref()?)?)
                .ok()?,
        })
    }

    fn emit_global_state_initialization(&mut self, plan: &GlobalStateInitialization<'_>) {
        self.non_leaf = true;
        self.frame_size = 16;
        self.output.pre_scheduled = true;
        self.output.instructions.extend([
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
        ]);
        self.emit_address_high(3, plan.aggregate);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, plan.first_value));
        self.emit_address_low(3, plan.aggregate);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: plan.first_offset,
        });
        self.record_relocation(RelocationKind::Rel24, plan.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.callee.to_owned(),
        });

        self.emit_address_high(5, plan.aggregate);
        self.emit_address_high(4, plan.scalar);
        self.emit_address_low(5, plan.aggregate);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, plan.scalar_high));
        self.output.instructions.push(Instruction::StoreWord {
            s: Eabi::general_result().number,
            a: 5,
            offset: plan.call_offset,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, plan.return_value));
        self.record_relocation(RelocationKind::Addr16Lo, plan.scalar);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 4,
            offset: 0,
        });
        self.output.instructions.extend([
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
