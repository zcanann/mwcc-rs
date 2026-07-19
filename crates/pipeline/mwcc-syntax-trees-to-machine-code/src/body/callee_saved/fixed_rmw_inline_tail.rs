//! Leaf fixed-register programs with semantically verified inline helper tails.
//!
//! The AR write/read helpers program seven halfword fields, then call a tiny
//! busy-wait and interrupt-clear helper. mwcc inlines both callees and schedules
//! the resulting poll/RMW tail as one leaf DAG. Recognition of the callees'
//! bodies lives in `inline_summaries`; this module owns only the call-site shape
//! and its cross-statement instruction schedule.

use super::fixed_rmw_recognize::*;
#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy)]
pub(super) enum DmaDirectionUpdate {
    Clear,
    Set,
}

impl Generator {
    pub(crate) fn try_fixed_rmw_with_inline_tail(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !function.guards.is_empty()
            || !function.locals.is_empty()
            || !self.frame_slots.is_empty()
            || function.return_type != Type::Void
            || function.return_expression.is_some()
            || function.parameters.len() != 3
            || function
                .parameters
                .iter()
                .any(|parameter| !matches!(parameter.parameter_type, Type::Int | Type::UnsignedInt))
        {
            return Ok(false);
        }
        let [main_address, aram_address, length] = function.parameters.as_slice() else {
            return Ok(false);
        };
        let [first @ Statement::Store { .. }, second @ Statement::Store { .. }, third @ Statement::Store { .. }, fourth @ Statement::Store { .. }, fifth @ Statement::Store { .. }, sixth @ Statement::Store { .. }, seventh @ Statement::Store { .. }, Statement::Expression(Expression::Call {
            name: poll_name,
            arguments: poll_arguments,
        }), Statement::Expression(Expression::Call {
            name: clear_name,
            arguments: clear_arguments,
        })] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if !poll_arguments.is_empty() || !clear_arguments.is_empty() {
            return Ok(false);
        }
        let Some(poll) = self.inline_summaries.fixed_poll(poll_name).cloned() else {
            return Ok(false);
        };
        let Some(clear) = self.inline_summaries.fixed_local_rmw(clear_name).cloned() else {
            return Ok(false);
        };
        if poll.bank != clear.bank || poll.index != clear.index {
            return Ok(false);
        }

        let statements = [first, second, third, fourth, fifth, sixth, seventh];
        let stores = statements.map(|statement| match statement {
            Statement::Store { target, value } => (target, value),
            _ => unreachable!(),
        });
        let mut slots = Vec::with_capacity(7);
        for (target, _) in stores {
            let Some(slot) = fixed_slot(target) else {
                return Ok(false);
            };
            slots.push(slot);
        }
        let bank = slots[0].0;
        if slots.iter().any(|(candidate, _)| *candidate != bank) || poll.bank != bank {
            return Ok(false);
        }
        let Some(&(base_address, element_type)) = self.fixed_address_arrays.get(bank) else {
            return Ok(false);
        };
        if element_type != Type::UnsignedShort {
            return Ok(false);
        }

        let mut parts = Vec::with_capacity(6);
        for index in [0usize, 1, 2, 3, 5, 6] {
            let Some(part) = rmw_parts(stores[index].0, stores[index].1) else {
                return Ok(false);
            };
            parts.push(part);
        }
        let Some(direction) = direct_direction_update(stores[4].0, stores[4].1) else {
            return Ok(false);
        };
        if parts.iter().map(|(mask, _)| *mask).collect::<Vec<_>>()
            != [-0x400, -0xffe1, -0x400, -0xffe1, -0x400, -0xffe1]
            || [0usize, 1, 2, 3, 5, 6].map(|index| match statements[index] {
                Statement::Store { value, .. } => masked_side_is_narrow(value),
                _ => unreachable!(),
            }) != [false; 6]
            || shifted_name(parts[0].1, 16) != Some(main_address.name.as_str())
            || low_half_name(parts[1].1) != Some(main_address.name.as_str())
            || shifted_name(parts[2].1, 16) != Some(aram_address.name.as_str())
            || low_half_name(parts[3].1) != Some(aram_address.name.as_str())
            || shifted_name(parts[4].1, 16) != Some(length.name.as_str())
            || low_half_name(parts[5].1) != Some(length.name.as_str())
        {
            return Ok(false);
        }

        // This captured leaf schedule assumes the ordinary EABI homes of three
        // general parameters. Requiring them explicitly makes future parameter
        // placement changes decline safely instead of corrupting the DAG.
        if self.lookup_general(&main_address.name) != Some(3)
            || self.lookup_general(&aram_address.name) != Some(4)
            || self.lookup_general(&length.name) != Some(5)
        {
            return Ok(false);
        }
        let Some((poll_begin, poll_end)) = rlwinm_mask(poll.mask as i64) else {
            return Ok(false);
        };

        let (high, low) = crate::expressions::split_address(base_address);
        let displacement = |index: i64| -> Compilation<i16> {
            i16::try_from(low as i64 + index * 2).map_err(|_| {
                Diagnostic::error("fixed-address inline RMW displacement is out of range")
            })
        };
        let mut offsets = Vec::with_capacity(7);
        for (_, index) in slots {
            offsets.push(displacement(index)?);
        }
        let tail_offset = displacement(poll.index)?;

        let legacy = self.behavior.fixed_address_rmw_style
            == mwcc_versions::FixedAddressRmwStyle::MaterializedPageWithPromotedMask;
        let materialize_poll_element = self.behavior.fixed_address_poll_address_style
            == mwcc_versions::FixedAddressPollAddressStyle::MaterializedElementForNonzeroIndex;
        // Seven-update DAG. Build 81+ hoists the poll element's address into r9
        // in the first store's latency window; build 53 retains r6 and folds
        // the element into the load displacement. r3/r4/r5 may be consumed as
        // scratch because both helper calls have been proven to inline.
        if legacy {
            self.emit_legacy_fixed_rmw_inline_tail(
                high,
                low,
                &offsets,
                tail_offset,
                direction,
                poll_begin,
                poll_end,
                clear.preserve_mask,
                clear.set_bits,
            );
        } else {
            self.output
                .instructions
                .push(Instruction::load_immediate_shifted(6, high));
            self.output
                .instructions
                .push(Instruction::ShiftRightLogicalImmediate {
                    a: 7,
                    s: 4,
                    shift: 16,
                });
            self.output
                .instructions
                .push(crate::expressions::displacement_load(
                    Pointee::UnsignedShort,
                    9,
                    6,
                    offsets[0],
                )?);
            self.output
                .instructions
                .push(Instruction::ShiftRightLogicalImmediate {
                    a: 0,
                    s: 3,
                    shift: 16,
                });
            self.output
                .instructions
                .push(Instruction::ClearLeftImmediate {
                    a: 8,
                    s: 3,
                    clear: 16,
                });
            self.output
                .instructions
                .push(Instruction::ShiftRightLogicalImmediate {
                    a: 3,
                    s: 5,
                    shift: 16,
                });
            self.output.instructions.push(Instruction::RotateAndMask {
                a: 9,
                s: 9,
                shift: 0,
                begin: 0,
                end: 21,
            });
            self.output
                .instructions
                .push(Instruction::ClearLeftImmediate {
                    a: 4,
                    s: 4,
                    clear: 16,
                });
            self.output
                .instructions
                .push(Instruction::Or { a: 9, s: 9, b: 0 });
            self.output
                .instructions
                .push(Instruction::ClearLeftImmediate {
                    a: 0,
                    s: 5,
                    clear: 16,
                });
            self.output
                .instructions
                .push(crate::expressions::displacement_store(
                    Pointee::UnsignedShort,
                    9,
                    6,
                    offsets[0],
                )?);
            if materialize_poll_element {
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 9,
                    a: 6,
                    immediate: tail_offset,
                });
            }

            self.output
                .instructions
                .push(crate::expressions::displacement_load(
                    Pointee::UnsignedShort,
                    5,
                    6,
                    offsets[1],
                )?);
            self.output.instructions.push(Instruction::RotateAndMask {
                a: 5,
                s: 5,
                shift: 0,
                begin: 27,
                end: 15,
            });
            self.output
                .instructions
                .push(Instruction::Or { a: 5, s: 5, b: 8 });
            self.output
                .instructions
                .push(crate::expressions::displacement_store(
                    Pointee::UnsignedShort,
                    5,
                    6,
                    offsets[1],
                )?);
            self.output
                .instructions
                .push(crate::expressions::displacement_load(
                    Pointee::UnsignedShort,
                    5,
                    6,
                    offsets[2],
                )?);
            self.output.instructions.push(Instruction::RotateAndMask {
                a: 5,
                s: 5,
                shift: 0,
                begin: 0,
                end: 21,
            });
            self.output
                .instructions
                .push(Instruction::Or { a: 5, s: 5, b: 7 });
            self.output
                .instructions
                .push(crate::expressions::displacement_store(
                    Pointee::UnsignedShort,
                    5,
                    6,
                    offsets[2],
                )?);
            self.output
                .instructions
                .push(crate::expressions::displacement_load(
                    Pointee::UnsignedShort,
                    5,
                    6,
                    offsets[3],
                )?);
            self.output.instructions.push(Instruction::RotateAndMask {
                a: 5,
                s: 5,
                shift: 0,
                begin: 27,
                end: 15,
            });
            self.output
                .instructions
                .push(Instruction::Or { a: 4, s: 5, b: 4 });
            self.output
                .instructions
                .push(crate::expressions::displacement_store(
                    Pointee::UnsignedShort,
                    4,
                    6,
                    offsets[3],
                )?);
            self.output
                .instructions
                .push(crate::expressions::displacement_load(
                    Pointee::UnsignedShort,
                    4,
                    6,
                    offsets[4],
                )?);
            match direction {
                DmaDirectionUpdate::Clear => {
                    self.output
                        .instructions
                        .push(Instruction::ClearLeftImmediate {
                            a: 4,
                            s: 4,
                            clear: 17,
                        })
                }
                DmaDirectionUpdate::Set => {
                    self.output.instructions.push(Instruction::OrImmediate {
                        a: 4,
                        s: 4,
                        immediate: 0x8000,
                    })
                }
            }
            self.output
                .instructions
                .push(crate::expressions::displacement_store(
                    Pointee::UnsignedShort,
                    4,
                    6,
                    offsets[4],
                )?);
            self.output
                .instructions
                .push(crate::expressions::displacement_load(
                    Pointee::UnsignedShort,
                    4,
                    6,
                    offsets[5],
                )?);
            self.output.instructions.push(Instruction::RotateAndMask {
                a: 4,
                s: 4,
                shift: 0,
                begin: 0,
                end: 21,
            });
            self.output
                .instructions
                .push(Instruction::Or { a: 3, s: 4, b: 3 });
            self.output
                .instructions
                .push(crate::expressions::displacement_store(
                    Pointee::UnsignedShort,
                    3,
                    6,
                    offsets[5],
                )?);
            self.output
                .instructions
                .push(crate::expressions::displacement_load(
                    Pointee::UnsignedShort,
                    3,
                    6,
                    offsets[6],
                )?);
            self.output.instructions.push(Instruction::RotateAndMask {
                a: 3,
                s: 3,
                shift: 0,
                begin: 27,
                end: 15,
            });
            self.output
                .instructions
                .push(Instruction::Or { a: 0, s: 3, b: 0 });
            self.output
                .instructions
                .push(crate::expressions::displacement_store(
                    Pointee::UnsignedShort,
                    0,
                    6,
                    offsets[6],
                )?);

            let loop_top = self.output.instructions.len();
            self.output
                .instructions
                .push(Instruction::LoadHalfwordZero {
                    d: 0,
                    a: if materialize_poll_element { 9 } else { 6 },
                    offset: if materialize_poll_element {
                        0
                    } else {
                        tail_offset
                    },
                });
            self.output
                .instructions
                .push(Instruction::RotateAndMaskRecord {
                    a: 0,
                    s: 0,
                    shift: 0,
                    begin: poll_begin,
                    end: poll_end,
                });
            self.output
                .instructions
                .push(Instruction::BranchConditionalForward {
                    options: 4,
                    condition_bit: 2,
                    target: loop_top,
                });

            self.output
                .instructions
                .push(Instruction::load_immediate_shifted(4, high));
            self.output
                .instructions
                .push(Instruction::load_immediate(0, clear.preserve_mask));
            self.output
                .instructions
                .push(crate::expressions::displacement_load(
                    Pointee::UnsignedShort,
                    3,
                    4,
                    tail_offset,
                )?);
            self.output
                .instructions
                .push(Instruction::And { a: 0, s: 3, b: 0 });
            self.output.instructions.push(Instruction::OrImmediate {
                a: 0,
                s: 0,
                immediate: clear.set_bits,
            });
            self.output
                .instructions
                .push(crate::expressions::displacement_store(
                    Pointee::UnsignedShort,
                    0,
                    4,
                    tail_offset,
                )?);
        }
        // The inlined loop contributes the same internal label family in every
        // measured compiler build. Refine this count from whole-object evidence
        // if another helper composition introduces additional anonymous nodes.
        self.output.anonymous_label_bump += 4;
        self.emit_epilogue_and_return();
        Ok(true)
    }
}

fn direct_direction_update(target: &Expression, value: &Expression) -> Option<DmaDirectionUpdate> {
    let Expression::Binary {
        operator,
        left,
        right,
    } = peel_casts(value)
    else {
        return None;
    };
    let constant = if same_operand(target, left) {
        constant_value(right)?
    } else if *operator == BinaryOperator::BitOr && same_operand(target, right) {
        constant_value(left)?
    } else {
        return None;
    };
    match operator {
        BinaryOperator::BitAnd if constant as u16 == 0x7fff => Some(DmaDirectionUpdate::Clear),
        BinaryOperator::BitOr if constant as u16 == 0x8000 => Some(DmaDirectionUpdate::Set),
        _ => None,
    }
}
