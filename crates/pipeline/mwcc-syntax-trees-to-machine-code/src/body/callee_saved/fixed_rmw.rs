//! Interrupt-protected fixed-address register programming.
//!
//! SDK DMA setup routines preserve parameters across an entering call, then
//! update several fields in one memory-mapped register bank before a leaving
//! call. mwcc schedules the independent field extracts into load-latency slots;
//! this family owns that cross-statement schedule separately from the generic
//! expression emitter.

use super::fixed_rmw_recognize::*;
#[allow(unused_imports)]
use super::*;

impl Generator {
    /// The AI DMA triple:
    ///
    /// ```text
    /// state = enter();
    /// bank[a] = (bank[a] & ~0x03ff) | (address >> 16);
    /// bank[b] = (bank[b] & ~0xffe0) | (address & 0xffff);
    /// bank[c] = (bank[c] & ~0x7fff) | ((length >> 5) & 0xffff);
    /// leave(state);
    /// ```
    ///
    /// The semantic recognizer is deliberately separate from emission. This is
    /// the three-node member of a larger fixed-RMW scheduling family (ARStartDMA
    /// is the seven-node sibling); extending it means adding a schedule, not
    /// weakening the generic statement emitter's byte-exact guarantees.
    pub(crate) fn try_interrupt_protected_fixed_rmw(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.try_interrupt_protected_fixed_rmw_triple(function)? {
            return Ok(true);
        }
        self.try_interrupt_protected_fixed_rmw_seven(function)
    }

    fn try_interrupt_protected_fixed_rmw_triple(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function.return_type != Type::Void
            || function.return_expression.is_some()
            || function.parameters.len() != 2
            || function.locals.len() != 1
        {
            return Ok(false);
        }
        let [address_parameter, length_parameter] = function.parameters.as_slice() else {
            return Ok(false);
        };
        if !matches!(
            address_parameter.parameter_type,
            Type::Int | Type::UnsignedInt
        ) || !matches!(
            length_parameter.parameter_type,
            Type::Int | Type::UnsignedInt
        ) {
            return Ok(false);
        }
        let [state] = function.locals.as_slice() else {
            return Ok(false);
        };
        if !matches!(state.declared_type, Type::Int | Type::UnsignedInt)
            || state.array_length.is_some()
            || state.is_static
            || state.initializer.is_some()
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
        }, first @ Statement::Store { .. }, second @ Statement::Store { .. }, third @ Statement::Store { .. }, Statement::Expression(Expression::Call {
            name: leave,
            arguments: leave_arguments,
        })] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if state_name != &state.name
            || !enter_arguments.is_empty()
            || !matches!(leave_arguments.as_slice(), [Expression::Variable(name)] if name == &state.name)
        {
            return Ok(false);
        }

        let stores = [first, second, third].map(|statement| match statement {
            Statement::Store { target, value } => (target, value),
            _ => unreachable!(),
        });
        let Some((bank, first_index)) = fixed_slot(stores[0].0) else {
            return Ok(false);
        };
        let Some((second_bank, second_index)) = fixed_slot(stores[1].0) else {
            return Ok(false);
        };
        let Some((third_bank, third_index)) = fixed_slot(stores[2].0) else {
            return Ok(false);
        };
        if second_bank != bank || third_bank != bank {
            return Ok(false);
        }
        let Some(&(base_address, element_type)) = self.fixed_address_arrays.get(bank) else {
            return Ok(false);
        };
        if element_type != Type::UnsignedShort {
            return Ok(false);
        }

        let Some((first_mask, first_insert)) = rmw_parts(stores[0].0, stores[0].1) else {
            return Ok(false);
        };
        let Some((second_mask, second_insert)) = rmw_parts(stores[1].0, stores[1].1) else {
            return Ok(false);
        };
        let Some((third_mask, third_insert)) = rmw_parts(stores[2].0, stores[2].1) else {
            return Ok(false);
        };
        if first_mask != -0x400
            || second_mask != -0xffe1
            || third_mask != -0x8000
            || shifted_name(first_insert, 16) != Some(address_parameter.name.as_str())
            || low_half_name(second_insert) != Some(address_parameter.name.as_str())
            || shifted_low_half_name(third_insert, 5) != Some(length_parameter.name.as_str())
        {
            return Ok(false);
        }

        let Some(address_incoming) = self.lookup_general(&address_parameter.name) else {
            return Ok(false);
        };
        let Some(length_incoming) = self.lookup_general(&length_parameter.name) else {
            return Ok(false);
        };
        let length_home = self.fresh_virtual_general();
        let address_home = self.fresh_virtual_general();
        let homes = vec![length_home, address_home];
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        self.non_leaf = true;
        self.frame_size = plan.frame_size;
        self.callee_saved = homes;
        self.output
            .instructions
            .extend(plan.prologue_interleaved(&[length_incoming, address_incoming]));
        self.emit_call(enter, enter_arguments, None, false)?;

        let address = u32::try_from(base_address)
            .map_err(|_| Diagnostic::error("fixed-address register bank is out of range"))?;
        let (high, low) = crate::expressions::split_address(address);
        let displacement = |index: i64| -> Compilation<i16> {
            i16::try_from(low as i64 + index * 2)
                .map_err(|_| Diagnostic::error("fixed-address RMW displacement is out of range"))
        };
        let first_offset = displacement(first_index)?;
        let second_offset = displacement(second_index)?;
        let third_offset = displacement(third_index)?;

        // Captured three-update DAG. r3 is intentionally absent: it holds the
        // entering call's result for `leave`. Independent extracts fill the
        // first two load-latency windows, and r0 carries the final inserted field.
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(7, high));
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 5,
                s: address_home,
                shift: 16,
            });
        self.output
            .instructions
            .push(crate::expressions::displacement_load(
                Pointee::UnsignedShort,
                6,
                7,
                first_offset,
            )?);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 4,
                s: address_home,
                clear: 16,
            });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 0,
            s: length_home,
            shift: 27,
            begin: 16,
            end: 31,
        });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 6,
            s: 6,
            shift: 0,
            begin: 0,
            end: 21,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 5, s: 6, b: 5 });
        self.output
            .instructions
            .push(crate::expressions::displacement_store(
                Pointee::UnsignedShort,
                5,
                7,
                first_offset,
            )?);
        self.output
            .instructions
            .push(crate::expressions::displacement_load(
                Pointee::UnsignedShort,
                5,
                7,
                second_offset,
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
                7,
                second_offset,
            )?);
        self.output
            .instructions
            .push(crate::expressions::displacement_load(
                Pointee::UnsignedShort,
                4,
                7,
                third_offset,
            )?);
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 4,
            s: 4,
            shift: 0,
            begin: 0,
            end: 16,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 4, b: 0 });
        self.output
            .instructions
            .push(crate::expressions::displacement_store(
                Pointee::UnsignedShort,
                0,
                7,
                third_offset,
            )?);

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
        self.emit_epilogue_and_return();
        Ok(true)
    }

    /// The ARAM DMA seven-update sibling. Its first four and final two updates
    /// narrow the preserved halfword before ORing, while the direction update
    /// narrows the combined result. mwcc exploits those shapes with two mask
    /// idioms and a rotate-mask-insert, and hoists all parameter extracts around
    /// the first load before committing the source-order stores.
    fn try_interrupt_protected_fixed_rmw_seven(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || function.return_type != Type::Void
            || function.return_expression.is_some()
            || function.parameters.len() != 4
            || function.locals.len() != 1
            || function
                .parameters
                .iter()
                .any(|parameter| !matches!(parameter.parameter_type, Type::Int | Type::UnsignedInt))
        {
            return Ok(false);
        }
        let [direction, main_address, aram_address, length] = function.parameters.as_slice() else {
            return Ok(false);
        };
        let [state] = function.locals.as_slice() else {
            return Ok(false);
        };
        if !matches!(state.declared_type, Type::Int | Type::UnsignedInt)
            || state.array_length.is_some()
            || state.is_static
            || state.initializer.is_some()
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
        }, first @ Statement::Store { .. }, second @ Statement::Store { .. }, third @ Statement::Store { .. }, fourth @ Statement::Store { .. }, fifth @ Statement::Store { .. }, sixth @ Statement::Store { .. }, seventh @ Statement::Store { .. }, Statement::Expression(Expression::Call {
            name: leave,
            arguments: leave_arguments,
        })] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        if state_name != &state.name
            || !enter_arguments.is_empty()
            || !matches!(leave_arguments.as_slice(), [Expression::Variable(name)] if name == &state.name)
        {
            return Ok(false);
        }

        let statements = [first, second, third, fourth, fifth, sixth, seventh];
        let stores = statements.map(|statement| match statement {
            Statement::Store { target, value } => (target, value),
            _ => unreachable!(),
        });
        let mut slots = Vec::with_capacity(stores.len());
        let mut parts = Vec::with_capacity(stores.len());
        for (target, value) in stores {
            let Some(slot) = fixed_slot(target) else {
                return Ok(false);
            };
            let Some(part) = rmw_parts(target, value) else {
                return Ok(false);
            };
            slots.push(slot);
            parts.push(part);
        }
        let bank = slots[0].0;
        if slots.iter().any(|(candidate, _)| *candidate != bank) {
            return Ok(false);
        }
        let Some(&(base_address, element_type)) = self.fixed_address_arrays.get(bank) else {
            return Ok(false);
        };
        if element_type != Type::UnsignedShort
            || parts.iter().map(|(mask, _)| *mask).collect::<Vec<_>>()
                != [-0x400, -0xffe1, -0x400, -0xffe1, -0x8001, -0x400, -0xffe1]
            || statements.map(|statement| match statement {
                Statement::Store { value, .. } => masked_side_is_narrow(value),
                _ => unreachable!(),
            }) != [true, true, true, true, false, true, true]
            || shifted_name(parts[0].1, 16) != Some(main_address.name.as_str())
            || low_half_name(parts[1].1) != Some(main_address.name.as_str())
            || shifted_name(parts[2].1, 16) != Some(aram_address.name.as_str())
            || low_half_name(parts[3].1) != Some(aram_address.name.as_str())
            || shifted_left_name(parts[4].1, 15) != Some(direction.name.as_str())
            || shifted_name(parts[5].1, 16) != Some(length.name.as_str())
            || low_half_name(parts[6].1) != Some(length.name.as_str())
        {
            return Ok(false);
        }

        let mut incoming = Vec::with_capacity(4);
        for parameter in [length, aram_address, main_address, direction] {
            let Some(register) = self.lookup_general(&parameter.name) else {
                return Ok(false);
            };
            incoming.push(register);
        }
        let homes: Vec<u8> = (0..4).map(|_| self.fresh_virtual_general()).collect();
        let length_home = homes[0];
        let aram_home = homes[1];
        let main_home = homes[2];
        let direction_home = homes[3];
        let plan = mwcc_vreg::FramePlan::sized_for(homes.clone());
        self.non_leaf = true;
        self.frame_size = plan.frame_size;
        self.callee_saved = homes;
        self.output
            .instructions
            .extend(plan.prologue_interleaved(&incoming));
        self.emit_call(enter, enter_arguments, None, false)?;

        let address = u32::try_from(base_address)
            .map_err(|_| Diagnostic::error("fixed-address register bank is out of range"))?;
        let (high, low) = crate::expressions::split_address(address);
        let mut offsets = Vec::with_capacity(slots.len());
        for (_, index) in slots {
            offsets.push(i16::try_from(low as i64 + index * 2).map_err(|_| {
                Diagnostic::error("fixed-address RMW displacement is out of range")
            })?);
        }

        // Captured seven-update DAG. The physical temporaries encode mwcc's
        // cross-statement schedule; the four call-crossing inputs remain virtual
        // and are colored onto r31..r28 by the shared allocator.
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(4, high));
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 0,
                s: main_home,
                shift: 16,
            });
        self.output
            .instructions
            .push(crate::expressions::displacement_load(
                Pointee::UnsignedShort,
                5,
                4,
                offsets[0],
            )?);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 9,
                s: main_home,
                clear: 16,
            });
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 8,
                s: aram_home,
                shift: 16,
            });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 7,
                s: aram_home,
                clear: 16,
            });
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 5,
            s: 5,
            shift: 0,
            begin: 16,
            end: 21,
        });
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 6,
                s: direction_home,
                shift: 15,
            });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 5, b: 0 });
        self.output
            .instructions
            .push(crate::expressions::displacement_store(
                Pointee::UnsignedShort,
                0,
                4,
                offsets[0],
            )?);
        self.output
            .instructions
            .push(Instruction::ShiftRightLogicalImmediate {
                a: 5,
                s: length_home,
                shift: 16,
            });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 0,
                s: length_home,
                clear: 16,
            });

        self.output
            .instructions
            .push(crate::expressions::displacement_load(
                Pointee::UnsignedShort,
                10,
                4,
                offsets[1],
            )?);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 10,
                s: 10,
                clear: 27,
            });
        self.output
            .instructions
            .push(Instruction::Or { a: 9, s: 10, b: 9 });
        self.output
            .instructions
            .push(crate::expressions::displacement_store(
                Pointee::UnsignedShort,
                9,
                4,
                offsets[1],
            )?);
        self.output
            .instructions
            .push(crate::expressions::displacement_load(
                Pointee::UnsignedShort,
                9,
                4,
                offsets[2],
            )?);
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 9,
            s: 9,
            shift: 0,
            begin: 16,
            end: 21,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 8, s: 9, b: 8 });
        self.output
            .instructions
            .push(crate::expressions::displacement_store(
                Pointee::UnsignedShort,
                8,
                4,
                offsets[2],
            )?);
        self.output
            .instructions
            .push(crate::expressions::displacement_load(
                Pointee::UnsignedShort,
                8,
                4,
                offsets[3],
            )?);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 8,
                s: 8,
                clear: 27,
            });
        self.output
            .instructions
            .push(Instruction::Or { a: 7, s: 8, b: 7 });
        self.output
            .instructions
            .push(crate::expressions::displacement_store(
                Pointee::UnsignedShort,
                7,
                4,
                offsets[3],
            )?);
        self.output
            .instructions
            .push(crate::expressions::displacement_load(
                Pointee::UnsignedShort,
                7,
                4,
                offsets[4],
            )?);
        self.output
            .instructions
            .push(Instruction::RotateAndMaskInsert {
                a: 6,
                s: 7,
                shift: 0,
                begin: 17,
                end: 31,
            });
        self.output
            .instructions
            .push(crate::expressions::displacement_store(
                Pointee::UnsignedShort,
                6,
                4,
                offsets[4],
            )?);
        self.output
            .instructions
            .push(crate::expressions::displacement_load(
                Pointee::UnsignedShort,
                6,
                4,
                offsets[5],
            )?);
        self.output.instructions.push(Instruction::RotateAndMask {
            a: 6,
            s: 6,
            shift: 0,
            begin: 16,
            end: 21,
        });
        self.output
            .instructions
            .push(Instruction::Or { a: 5, s: 6, b: 5 });
        self.output
            .instructions
            .push(crate::expressions::displacement_store(
                Pointee::UnsignedShort,
                5,
                4,
                offsets[5],
            )?);
        self.output
            .instructions
            .push(crate::expressions::displacement_load(
                Pointee::UnsignedShort,
                5,
                4,
                offsets[6],
            )?);
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediate {
                a: 5,
                s: 5,
                clear: 27,
            });
        self.output
            .instructions
            .push(Instruction::Or { a: 0, s: 5, b: 0 });
        self.output
            .instructions
            .push(crate::expressions::displacement_store(
                Pointee::UnsignedShort,
                0,
                4,
                offsets[6],
            )?);

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
        self.emit_epilogue_and_return();
        Ok(true)
    }
}
