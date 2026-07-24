//! Value-producing postfix increment/decrement.
//!
//! A postfix step has two results with overlapping lifetimes: the expression
//! yields the old value while the lvalue receives the stepped value. Keeping
//! that split here prevents the ordinary assignment path from accidentally
//! returning the new value.

#[allow(unused_imports)]
use super::*;

impl Generator {
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
        let Some(&value_type) = self.globals.get(name.as_str()) else {
            return Err(Diagnostic::error(
                "a frame-resident postfix step used as a value is not supported yet (roadmap)",
            ));
        };
        if !matches!(
            value_type,
            Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. }
        ) {
            return Err(Diagnostic::error(
                "a postfix step value currently requires a word-sized integer or pointer global",
            ));
        }
        let amount = match value_type {
            Type::Pointer(pointee) => i16::from(pointee.size()),
            Type::StructPointer { element_size } => i16::try_from(element_size)
                .map_err(|_| Diagnostic::error("postfix pointer stride is out of range"))?,
            _ => 1,
        };
        let amount = signed_step_amount(operator, amount)?;
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
