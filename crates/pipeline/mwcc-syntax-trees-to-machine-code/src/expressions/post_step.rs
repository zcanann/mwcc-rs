//! Value-producing postfix increment/decrement.
//!
//! A postfix step has two results with overlapping lifetimes: the expression
//! yields the old value while the lvalue receives the stepped value. Keeping
//! that split here prevents the ordinary assignment path from accidentally
//! returning the new value.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Store through the old address produced by `*pointer++`, then advance the
    /// tracked pointer. Call-valued stores need a separately retained old/new
    /// pointer pair and remain with the general call-aware store scheduler.
    pub(crate) fn try_emit_post_step_pointer_store(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        let Expression::Dereference { pointer } = target else {
            return Ok(false);
        };
        let Expression::PostStep {
            target: pointer_target,
            operator,
            pointer_link,
        } = pointer.as_ref()
        else {
            return Ok(false);
        };
        if expression_has_call(value) {
            return Ok(false);
        }

        let (pointee, address) = self.pointer_leaf(pointer_target)?;
        let restore = address != GENERAL_SCRATCH && self.reserved.insert(address);
        let source = self.place_store_value(value, pointee)?;
        if restore {
            self.reserved.remove(&address);
        }
        self.output
            .instructions
            .push(displacement_store(pointee, source, address, 0)?);
        if !self.emit_post_step_update_after_use(
            pointer_target,
            *operator,
            *pointer_link,
        )? {
            return Err(Diagnostic::error(
                "a store through a postfix pointer needs a register-local target",
            ));
        }
        Ok(true)
    }

    /// Apply the mutation half of a postfix step after a caller has consumed
    /// the old register value. Member access through `pointer++` uses this to
    /// preserve MWCC's `load old; increment pointer` schedule without first
    /// copying the old pointer into a synthetic result register.
    pub(crate) fn emit_post_step_update_after_use(
        &mut self,
        target: &Expression,
        operator: BinaryOperator,
        pointer_link: Option<(u32, u32)>,
    ) -> Compilation<bool> {
        if pointer_link.is_some() {
            return Ok(false);
        }
        let Expression::Variable(name) = target else {
            return Ok(false);
        };
        let Some((source, class, width, pointee, stride)) =
            self.locations.get(name.as_str()).map(|location| {
                (
                    location.register,
                    location.class,
                    location.width,
                    location.pointee,
                    location.stride,
                )
            })
        else {
            return Ok(false);
        };
        if class != ValueClass::General || width != 32 {
            return Err(Diagnostic::error(
                "a register-local postfix update requires a word-sized integer or pointer",
            ));
        }
        let amount = stride
            .map(i16::try_from)
            .transpose()
            .map_err(|_| Diagnostic::error("postfix pointer stride is out of range"))?
            .or_else(|| pointee.map(|pointee| i16::from(pointee.size())))
            .unwrap_or(1);
        let amount = signed_step_amount(operator, amount)?;
        let stepped = if source < mwcc_vreg::VIRTUAL_BASE {
            self.fresh_virtual_general()
        } else {
            source
        };
        self.output.instructions.push(Instruction::AddImmediate {
            d: stepped,
            a: source,
            immediate: amount,
        });
        self.locations
            .get_mut(name.as_str())
            .expect("postfix local location disappeared")
            .register = stepped;
        Ok(true)
    }

    pub(crate) fn emit_post_step_value(
        &mut self,
        target: &Expression,
        operator: BinaryOperator,
        pointer_link: Option<(u32, u32)>,
        destination: u8,
    ) -> Compilation<()> {
        if pointer_link.is_some() {
            return Err(Diagnostic::error(
                "an overloaded iterator postfix step used as a value needs iterator lowering",
            ));
        }
        if let Expression::Member {
            base,
            offset,
            member_type,
            index_stride: None,
        } = target
        {
            let Expression::Variable(base_name) = base.as_ref() else {
                return Err(Diagnostic::error(
                    "a postfix member step requires a register-local base",
                ));
            };
            let Some(base_register) = self.lookup_general(base_name) else {
                return Err(Diagnostic::error(
                    "a postfix member step requires a register-local base",
                ));
            };
            let pointee = pointee_of_type(*member_type).ok_or_else(|| {
                Diagnostic::error("a postfix member step requires scalar storage")
            })?;
            if pointee.size() != 4 {
                return Err(Diagnostic::error(
                    "a postfix member step value requires a word-sized member",
                ));
            }
            let amount = step_amount_for_type(*member_type, operator)?;
            let old_value = if destination == base_register {
                self.fresh_virtual_general()
            } else {
                destination
            };
            self.output.instructions.push(displacement_load(
                pointee,
                old_value,
                base_register,
                i16::try_from(*offset)
                    .map_err(|_| Diagnostic::error("postfix member offset is out of range"))?,
            )?);
            let stepped = self.fresh_virtual_general();
            self.output.instructions.push(Instruction::AddImmediate {
                d: stepped,
                a: old_value,
                immediate: amount,
            });
            self.output.instructions.push(displacement_store(
                pointee,
                stepped,
                base_register,
                i16::try_from(*offset)
                    .map_err(|_| Diagnostic::error("postfix member offset is out of range"))?,
            )?);
            if old_value != destination {
                self.emit_integer_materialization_copy(destination, old_value);
            }
            return Ok(());
        }
        let Expression::Variable(name) = target else {
            return Err(Diagnostic::error(
                "a postfix step on this lvalue is not supported yet (roadmap)",
            ));
        };
        if let Some((source, class, width, pointee, stride)) =
            self.locations.get(name.as_str()).map(|location| {
                (
                    location.register,
                    location.class,
                    location.width,
                    location.pointee,
                    location.stride,
                )
            })
        {
            if class != ValueClass::General || width != 32 {
                return Err(Diagnostic::error(
                    "a register-local postfix step value requires a word-sized integer or pointer",
                ));
            }
            let amount = stride
                .map(i16::try_from)
                .transpose()
                .map_err(|_| Diagnostic::error("postfix pointer stride is out of range"))?
                .or_else(|| pointee.map(|pointee| i16::from(pointee.size())))
                .unwrap_or(1);
            let amount = signed_step_amount(operator, amount)?;

            // The old value belongs to the surrounding expression. A physical
            // entry register cannot also retain the stepped local across a
            // call, so split its new value into an allocatable lane. Once the
            // local already owns a virtual lane, update it in place.
            let fused_entry_copy = source != destination
                && self.output.instructions.last().is_some_and(|instruction| {
                    matches!(
                        instruction,
                        Instruction::Or { a, s, b }
                            if *a == source && *s == destination && *b == destination
                    ) || matches!(
                        instruction,
                        Instruction::AddImmediate { d, a, immediate: 0 }
                            if *d == source && *a == destination
                    )
                });
            if fused_entry_copy {
                self.output.instructions.pop();
                self.output.instructions.push(Instruction::AddImmediate {
                    d: source,
                    a: destination,
                    immediate: amount,
                });
                return Ok(());
            }
            if source != destination {
                self.emit_integer_materialization_copy(destination, source);
            }
            let stepped = if source < mwcc_vreg::VIRTUAL_BASE {
                self.fresh_virtual_general()
            } else {
                source
            };
            self.output.instructions.push(Instruction::AddImmediate {
                d: stepped,
                a: source,
                immediate: amount,
            });
            self.locations
                .get_mut(name.as_str())
                .expect("postfix local location disappeared")
                .register = stepped;
            return Ok(());
        }
        if let Some(slot) = self.frame_slots.get(name.as_str()).copied() {
            if slot.is_array || slot.class != ValueClass::General {
                return Err(Diagnostic::error(
                    "a frame-local postfix step value requires a word-sized integer or pointer",
                ));
            }
            let pointee = frame_value_pointee(slot.value_type).ok_or_else(|| {
                Diagnostic::error("a frame-local postfix step value requires a scalar storage type")
            })?;
            let amount = step_amount_for_type(slot.value_type, operator)?;
            let old_value = self.post_step_old_value_register(destination);
            self.output
                .instructions
                .push(displacement_load(pointee, old_value, 1, slot.offset)?);
            self.output.instructions.push(Instruction::AddImmediate {
                d: GENERAL_SCRATCH,
                a: old_value,
                immediate: amount,
            });
            let copy_after_store = self.behavior.materialization_copy_style
                == mwcc_versions::MaterializationCopyStyle::AddImmediateZero;
            if old_value != destination && !copy_after_store {
                self.output
                    .instructions
                    .push(Instruction::move_register(destination, old_value));
            }
            self.output.instructions.push(displacement_store(
                pointee,
                GENERAL_SCRATCH,
                1,
                slot.offset,
            )?);
            self.written_slots.insert(slot.offset);
            if old_value != destination && copy_after_store {
                self.output
                    .instructions
                    .push(Instruction::move_register(destination, old_value));
            }
            return Ok(());
        }

        let Some(&value_type) = self.globals.get(name.as_str()) else {
            return Err(Diagnostic::error(
                "a postfix step target has no register, frame, or global storage",
            ));
        };
        let amount = step_amount_for_type(value_type, operator)?;
        let old_value = self.post_step_old_value_register(destination);
        self.emit_global_load(name, old_value)?;
        self.output.instructions.push(Instruction::AddImmediate {
            d: GENERAL_SCRATCH,
            a: old_value,
            immediate: amount,
        });
        let copy_after_store = self.behavior.materialization_copy_style
            == mwcc_versions::MaterializationCopyStyle::AddImmediateZero;
        if old_value != destination && !copy_after_store {
            self.output
                .instructions
                .push(Instruction::move_register(destination, old_value));
        }
        self.emit_global_store(
            name,
            pointee_of_type(value_type).expect("word post-step type is scalar"),
            GENERAL_SCRATCH,
        )?;
        if old_value != destination && copy_after_store {
            self.output
                .instructions
                .push(Instruction::move_register(destination, old_value));
        }
        Ok(())
    }

    fn post_step_old_value_register(&mut self, destination: u8) -> u8 {
        let old_value = if destination >= 14 || mwcc_vreg::Reg::is_virtual_field(destination) {
            let mut avoid = Vec::with_capacity(self.reserved.len() + 1);
            avoid.push(GENERAL_SCRATCH);
            avoid.extend(self.reserved.iter().copied());
            avoid.sort_unstable();
            avoid.dedup();
            self.fresh_virtual_general_avoiding(avoid)
        } else {
            destination
        };
        old_value
    }
}

pub(crate) fn step_amount_for_type(
    value_type: Type,
    operator: BinaryOperator,
) -> Compilation<i16> {
    let amount = match value_type {
        Type::Int | Type::UnsignedInt => 1,
        Type::Pointer(pointee) => i16::from(pointee.size()),
        Type::StructPointer { element_size } => i16::try_from(element_size)
            .map_err(|_| Diagnostic::error("postfix pointer stride is out of range"))?,
        _ => {
            return Err(Diagnostic::error(
                "a postfix step value requires a word-sized integer or pointer",
            ))
        }
    };
    signed_step_amount(operator, amount)
}

fn signed_step_amount(operator: BinaryOperator, amount: i16) -> Compilation<i16> {
    match operator {
        BinaryOperator::Add => Ok(amount),
        BinaryOperator::Subtract => amount
            .checked_neg()
            .ok_or_else(|| Diagnostic::error("postfix pointer stride is out of range")),
        _ => Err(Diagnostic::error(
            "a postfix step requires increment or decrement",
        )),
    }
}
