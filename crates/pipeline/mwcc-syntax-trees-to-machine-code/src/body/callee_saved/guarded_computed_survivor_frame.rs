//! Frame-resident continuations after a guarded computed survivor.
//!
//! A stack array and a value kept in a callee-saved register share one physical
//! frame. This owner composes those storage classes, while the sibling prefix
//! module owns recognition and scheduling of the computed address and guard.

use super::guarded_computed_survivor::GuardedComputedSurvivor;
#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Lower a computed pointer, call-valued early return, straight-line frame
    /// continuation, and terminal word load through the surviving pointer.
    pub(crate) fn try_guarded_computed_survivor_frame(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !self.frame_slots.is_empty()
            || !function.guards.is_empty()
            || !matches!(function.return_type, Type::Int | Type::UnsignedInt)
        {
            return Ok(false);
        }
        let [pointer_local, array_local] = function.locals.as_slice() else {
            return Ok(false);
        };
        let Some(array_length) = array_local.array_length else {
            return Ok(false);
        };
        if array_local.is_static
            || array_local.initializer.is_some()
            || array_local.data_bytes.is_some()
            || !matches!(array_local.declared_type, Type::Char | Type::UnsignedChar)
        {
            return Ok(false);
        }
        let [Statement::Assign { name, value }, Statement::If {
            condition,
            then_body,
            else_body,
        }, continuation @ ..] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        let [Statement::Return(Some(guard_result))] = then_body.as_slice() else {
            return Ok(false);
        };
        if !else_body.is_empty()
            || continuation.is_empty()
            || continuation.iter().any(|statement| {
                !supports_frame_continuation_statement(statement, &array_local.name, array_length)
            })
        {
            return Ok(false);
        }
        let Some(return_expression) = function.return_expression.as_ref() else {
            return Ok(false);
        };
        let Some(shape) = GuardedComputedSurvivor::recognize_parts(
            pointer_local,
            name,
            value,
            condition,
            guard_result,
            return_expression,
            self,
        ) else {
            return Ok(false);
        };

        let array_bytes = u16::from(array_local.declared_type.width() / 8)
            .checked_mul(array_length)
            .filter(|bytes| *bytes != 0 && *bytes <= u16::from(u8::MAX));
        let Some(array_bytes) = array_bytes else {
            return Ok(false);
        };
        // In the linkage-first generation the local lane follows a four-byte
        // optimizer reservation; predecrement frames start locals directly
        // above the linkage area. The saved home occupies the final word.
        let array_offset = match self.behavior.frame_convention {
            FrameConvention::LinkageFirst => 12,
            FrameConvention::Predecrement => 8,
        };
        let occupied = i32::from(array_offset) + i32::from(array_bytes) + 4;
        let frame_size = i16::try_from((occupied + 15) / 16 * 16)
            .map_err(|_| Diagnostic::error("guarded computed survivor frame is too large"))?;
        self.frame_slots.insert(
            array_local.name.clone(),
            FrameSlot {
                offset: array_offset,
                class: ValueClass::General,
                size: array_bytes as u8,
                parameter_register: None,
                is_array: true,
            },
        );
        let array_pointee = match array_local.declared_type {
            Type::Char => Pointee::Char,
            Type::UnsignedChar => Pointee::UnsignedChar,
            _ => unreachable!("array element type was gated"),
        };
        self.locations.insert(
            array_local.name.clone(),
            Location {
                class: ValueClass::General,
                register: 0,
                signed: false,
                width: 32,
                pointee: Some(array_pointee),
                stride: None,
            },
        );
        if !self.emit_guarded_computed_survivor_prefix(&shape, frame_size)? {
            return Ok(false);
        }
        self.locations.insert(
            pointer_local.name.clone(),
            Location {
                class: ValueClass::General,
                register: 31,
                signed: false,
                width: 32,
                pointee: None,
                stride: Some(shape.stride),
            },
        );

        let success = self.fresh_label();
        let epilogue = self.fresh_label();
        self.emit_branch_conditional_to(4, 2, success); // bne continuation
        self.output
            .instructions
            .push(Instruction::load_immediate(3, shape.guard_result));
        self.emit_branch_to(epilogue);
        self.bind_label(success);
        let mut preloaded_call = false;
        for (statement_index, statement) in continuation.iter().enumerate() {
            match statement {
                Statement::Expression(Expression::Call { name, .. }) if preloaded_call => {
                    self.record_relocation(RelocationKind::Rel24, name);
                    self.output.instructions.push(Instruction::BranchAndLink {
                        target: name.clone(),
                    });
                    preloaded_call = false;
                }
                Statement::Expression(Expression::Call { .. }) => {
                    self.emit_statement(statement)?;
                }
                Statement::Store { target, value } => {
                    let Expression::Index { index, .. } = target else {
                        unreachable!("continuation store was gated")
                    };
                    let element = constant_value(index).expect("continuation index was gated");
                    let stored = constant_value(value).expect("continuation value was gated");
                    let stored = i16::try_from(stored).map_err(|_| {
                        Diagnostic::error("frame continuation byte constant is out of range")
                    })?;
                    self.output
                        .instructions
                        .push(Instruction::load_immediate(GENERAL_SCRATCH, stored));
                    let next_is_array_call = continuation
                        .get(statement_index + 1)
                        .is_some_and(|next| is_single_array_call(next, &array_local.name));
                    if self.behavior.frame_convention == FrameConvention::Predecrement
                        && next_is_array_call
                    {
                        self.output.instructions.push(Instruction::AddImmediate {
                            d: Eabi::FIRST_GENERAL_ARGUMENT,
                            a: 1,
                            immediate: array_offset,
                        });
                        preloaded_call = true;
                    }
                    self.output.instructions.push(Instruction::StoreByte {
                        s: GENERAL_SCRATCH,
                        a: 1,
                        offset: i16::try_from(i64::from(array_offset) + element).map_err(|_| {
                            Diagnostic::error("frame continuation byte offset is out of range")
                        })?,
                    });
                }
                _ => unreachable!("continuation statement was gated"),
            }
        }
        debug_assert!(!preloaded_call);
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 31,
            offset: shape.member_offset,
        });
        self.bind_label(epilogue);
        self.output.anonymous_label_bump += 2;
        self.emit_epilogue_and_return();
        Ok(true)
    }
}

fn is_single_array_call(statement: &Statement, array_name: &str) -> bool {
    matches!(
        statement,
        Statement::Expression(Expression::Call { arguments, .. })
            if matches!(arguments.as_slice(), [Expression::Variable(name)] if name == array_name)
    )
}

fn supports_frame_continuation_statement(
    statement: &Statement,
    array_name: &str,
    array_length: u16,
) -> bool {
    match statement {
        Statement::Expression(Expression::Call { .. }) => true,
        Statement::Store {
            target: Expression::Index { base, index },
            value,
        } => {
            matches!(base.as_ref(), Expression::Variable(name) if name == array_name)
                && constant_value(index)
                    .is_some_and(|index| index >= 0 && index < i64::from(array_length))
                && constant_value(value).is_some()
        }
        _ => false,
    }
}
