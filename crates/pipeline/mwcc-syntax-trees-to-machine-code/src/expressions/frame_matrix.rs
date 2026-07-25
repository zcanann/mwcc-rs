//! Address resolution for flattened multidimensional automatic arrays.

#[allow(unused_imports)]
use super::*;

impl Generator {
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
