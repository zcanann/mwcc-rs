//! CodeWarrior bit-field allocation-unit layout.
//!
//! Adjacent fields share one unit even when their declared integer types differ.
//! The unit grows to the widest participating type; it closes only when the next
//! field no longer fits. Ordinary members trim the open unit to the bytes whose
//! bits were actually consumed.

use super::{type_alignment, type_size};
use crate::items::types::{advance_layout_offset, align_layout_offset};
use mwcc_core::{Compilation, Diagnostic};
use mwcc_syntax_trees::Type;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct BitFieldUnit {
    storage_type: Type,
    offset: u32,
    bits_used: u8,
}

impl BitFieldUnit {
    fn capacity_bits(self) -> u8 {
        (type_size(self.storage_type) * 8) as u8
    }

    fn trimmed_end(self) -> u32 {
        self.offset + u32::from(self.bits_used).div_ceil(8)
    }
}

pub(super) fn close_bit_field_unit(unit: &mut Option<BitFieldUnit>, offset: &mut u32) {
    if let Some(unit) = unit.take() {
        *offset = unit.trimmed_end();
    }
}

pub(super) fn place_bit_field(
    unit: &mut Option<BitFieldUnit>,
    offset: &mut u32,
    alignment_max: &mut u32,
    field_type: Type,
    width: u8,
    requested_alignment: u32,
) -> Compilation<(u32, u8)> {
    let field_bits = (type_size(field_type) * 8) as u8;
    if width == 0 || width > field_bits {
        return Err(Diagnostic::error(
            "an unsupported bit-field width (roadmap)",
        ));
    }

    let field_alignment = type_alignment(field_type).max(1).max(requested_alignment);
    *alignment_max = (*alignment_max).max(field_alignment);

    if let Some(active) = *unit {
        let capacity_bits = active.capacity_bits().max(field_bits);
        if active.bits_used + width <= capacity_bits {
            let storage_type = if field_bits > active.capacity_bits() {
                field_type
            } else {
                active.storage_type
            };
            let bit_offset = active.bits_used;
            let expanded = BitFieldUnit {
                storage_type,
                offset: active.offset,
                bits_used: active.bits_used + width,
            };
            // Keep `offset` at the full allocation-unit extent. A following
            // ordinary field will trim it; an aggregate ending here retains
            // the complete unit before trailing alignment.
            *offset = advance_layout_offset(active.offset, type_size(storage_type))?;
            *unit = Some(expanded);
            return Ok((active.offset, bit_offset));
        }
        *offset = active.trimmed_end();
    }

    let unit_offset = align_layout_offset(*offset, field_alignment)?;
    *offset = advance_layout_offset(unit_offset, type_size(field_type))?;
    *unit = Some(BitFieldUnit {
        storage_type: field_type,
        offset: unit_offset,
        bits_used: width,
    });
    Ok((unit_offset, 0))
}
