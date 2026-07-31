//! Address resolution for flattened multidimensional automatic arrays.

#[allow(unused_imports)]
use super::*;

fn loaded_row_index(expression: &Expression) -> bool {
    matches!(
        expression,
        Expression::Member {
            member_type: Type::Char
                | Type::UnsignedChar
                | Type::Short
                | Type::UnsignedShort
                | Type::Int
                | Type::UnsignedInt,
            index_stride: None,
            ..
        }
    )
}

impl Generator {
    pub(crate) fn emit_frame_matrix_row_address(
        &mut self,
        name: &str,
        row: &Expression,
        destination: u8,
    ) -> Compilation<()> {
        let slot = self.frame_slots.get(name).copied().ok_or_else(|| {
            Diagnostic::error(format!("flattened frame matrix '{name}' has no frame slot"))
        })?;
        let row_bytes = *self.frame_row_bytes.get(name).ok_or_else(|| {
            Diagnostic::error(format!("flattened frame matrix '{name}' has no row stride"))
        })?;
        if let Some(row) = constant_value(row) {
            let offset = row
                .checked_mul(i64::from(row_bytes))
                .and_then(|offset| offset.checked_add(i64::from(slot.offset)))
                .and_then(|offset| i16::try_from(offset).ok())
                .ok_or_else(|| Diagnostic::error("frame matrix row address is out of range"))?;
            self.output.instructions.push(Instruction::AddImmediate {
                d: destination,
                a: 1,
                immediate: offset,
            });
            return Ok(());
        }

        let row_register = match self.general_register_of_leaf(row) {
            Ok(register) => register,
            Err(_) if loaded_row_index(row) && destination != GENERAL_SCRATCH => {
                self.evaluate_general(row, GENERAL_SCRATCH)?;
                if row_bytes.is_power_of_two() {
                    self.output
                        .instructions
                        .push(Instruction::ShiftLeftImmediate {
                            a: GENERAL_SCRATCH,
                            s: GENERAL_SCRATCH,
                            shift: row_bytes.trailing_zeros() as u8,
                        });
                } else {
                    self.output.instructions.push(Instruction::MultiplyImmediate {
                        d: GENERAL_SCRATCH,
                        a: GENERAL_SCRATCH,
                        immediate: i16::try_from(row_bytes).map_err(|_| {
                            Diagnostic::error("frame matrix row stride is out of range")
                        })?,
                    });
                }
                self.output.instructions.push(Instruction::AddImmediate {
                    d: destination,
                    a: 1,
                    immediate: slot.offset,
                });
                self.output.instructions.push(Instruction::Add {
                    d: destination,
                    a: destination,
                    b: GENERAL_SCRATCH,
                });
                return Ok(());
            }
            Err(diagnostic) => return Err(diagnostic),
        };
        if row_bytes.is_power_of_two() {
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: destination,
                    s: row_register,
                    shift: row_bytes.trailing_zeros() as u8,
                });
        } else {
            self.output.instructions.push(Instruction::MultiplyImmediate {
                d: destination,
                a: row_register,
                immediate: i16::try_from(row_bytes).map_err(|_| {
                    Diagnostic::error("frame matrix row stride is out of range")
                })?,
            });
        }
        let base = if destination == GENERAL_SCRATCH {
            self.fresh_virtual_general()
        } else {
            GENERAL_SCRATCH
        };
        self.output.instructions.push(Instruction::AddImmediate {
            d: base,
            a: 1,
            immediate: slot.offset,
        });
        self.output.instructions.push(Instruction::Add {
            d: destination,
            a: base,
            b: destination,
        });
        Ok(())
    }

    /// Resolve `matrix[row][column]` to its scalar element and r1-relative
    /// displacement. The parser flattens the allocation but retains the source
    /// row width, so neither load nor store needs to materialize the row pointer
    /// when both indices are constant.
    pub(crate) fn frame_matrix_element(
        &self,
        base: &Expression,
        column: &Expression,
    ) -> Compilation<Option<(Pointee, i16)>> {
        let Expression::Index {
            base: row_base,
            index: row,
        } = base
        else {
            return Ok(None);
        };
        let Expression::Variable(name) = row_base.as_ref() else {
            return Ok(None);
        };
        let (Some(row), Some(column)) = (constant_value(row), constant_value(column)) else {
            return Ok(None);
        };
        let slot = self.frame_slots.get(name).copied().ok_or_else(|| {
            Diagnostic::error(format!("flattened frame matrix '{name}' has no frame slot"))
        })?;
        let row_bytes = *self.frame_row_bytes.get(name).ok_or_else(|| {
            Diagnostic::error(format!("flattened frame matrix '{name}' has no row stride"))
        })?;
        let element = *self.frame_row_pointees.get(name).ok_or_else(|| {
            Diagnostic::error(format!("flattened frame matrix '{name}' has no element type"))
        })?;
        let displacement = row
            .checked_mul(i64::from(row_bytes))
            .and_then(|offset| {
                column
                    .checked_mul(i64::from(element.size()))
                    .and_then(|column| offset.checked_add(column))
            })
            .and_then(|offset| offset.checked_add(i64::from(slot.offset)))
            .and_then(|offset| i16::try_from(offset).ok())
            .ok_or_else(|| Diagnostic::error("frame matrix subscript is out of range"))?;
        Ok(Some((element, displacement)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loaded_scalar_members_are_computed_row_indices() {
        let member = Expression::Member {
            base: Box::new(Expression::Variable("data".into())),
            offset: 1,
            member_type: Type::UnsignedChar,
            index_stride: None,
        };

        assert!(loaded_row_index(&member));
        assert!(!loaded_row_index(&Expression::IntegerLiteral(1)));
    }
}
