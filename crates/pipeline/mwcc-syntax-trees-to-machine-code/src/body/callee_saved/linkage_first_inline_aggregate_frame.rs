//! Linkage-first placement for aggregate locals below retained inline lanes.
//!
//! A body with leading materialized aggregate initializers can acquire later
//! value-inline residue after its addressable locals have already been laid
//! out. Build 163 retains one lane per leading initializer image, places those
//! lanes below the complete local region, and orders the generated images by
//! source identity. Keeping this policy separate from general frame
//! reconciliation prevents every nested value composition from inflating the
//! frame independently.

use crate::generator::FrameSlot;
use mwcc_syntax_trees::Type;
use std::collections::HashMap;

pub(super) struct InlineAggregateFramePlan {
    pub(super) prefix_bytes: i16,
    pub(super) slot_moves: Vec<(String, i16, i16, i16)>,
    pub(super) scratch_start: i16,
    pub(super) scratch_end: i16,
}

pub(super) fn plan(
    slots: &HashMap<String, FrameSlot>,
    initial_inline_bytes: usize,
    total_inline_bytes: usize,
    entry_lane_bytes: i16,
    saved_registers: usize,
    float_to_int_scratch_end: i16,
    int_to_float_scratch_next: i16,
    int_to_float_scratch_end: i16,
) -> Option<InlineAggregateFramePlan> {
    if initial_inline_bytes % 8 != 0
        || total_inline_bytes <= initial_inline_bytes
        || entry_lane_bytes != 8
        || saved_registers != 2
        || float_to_int_scratch_end != 0
        || int_to_float_scratch_end == 0
        || int_to_float_scratch_next != int_to_float_scratch_end
    {
        return None;
    }

    let mut ordered = slots.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|(_, slot)| slot.offset);
    let mut cursor = 8i16;
    for (_, slot) in &ordered {
        if slot.offset != cursor
            || slot.is_array
            || slot.parameter_register.is_some()
            || !matches!(slot.value_type, Type::Struct { .. })
        {
            return None;
        }
        cursor = cursor.checked_add(i16::try_from(slot.size).ok()?)?;
    }
    if int_to_float_scratch_end != cursor.checked_add(8)? {
        return None;
    }

    let mut generated = ordered
        .iter()
        .filter(|(name, slot)| {
            temporary_storage_ordinal(name).is_some() && slot.size == 8
        })
        .map(|(name, slot)| ((*name).clone(), slot.offset))
        .collect::<Vec<_>>();
    if generated.len() != 2 || ordered.len().saturating_sub(generated.len()) < 3 {
        return None;
    }
    generated.sort_by_key(|(name, _)| temporary_storage_ordinal(name));
    let mut generated_offsets = generated
        .iter()
        .map(|(_, offset)| *offset)
        .collect::<Vec<_>>();
    generated_offsets.sort_unstable();

    let initializer_lane_bytes = generated.len().checked_mul(8)?;
    let retained_inline_bytes = initial_inline_bytes.max(initializer_lane_bytes);
    if total_inline_bytes <= retained_inline_bytes {
        return None;
    }
    let prefix_bytes = entry_lane_bytes
        .checked_add(i16::try_from(retained_inline_bytes).ok()?)?;
    let slot_moves = ordered
        .into_iter()
        .map(|(name, slot)| {
            let new_offset = generated
                .iter()
                .position(|(generated_name, _)| generated_name == name)
                .map(|index| generated_offsets[index])
                .unwrap_or(slot.offset)
                .checked_add(prefix_bytes)?;
            Some((
                name.clone(),
                slot.offset,
                i16::try_from(slot.size).ok()?,
                new_offset,
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    Some(InlineAggregateFramePlan {
        prefix_bytes,
        slot_moves,
        scratch_start: cursor,
        scratch_end: int_to_float_scratch_end,
    })
}

fn temporary_storage_ordinal(name: &str) -> Option<usize> {
    name.strip_prefix("__mwcc_temporary_storage_")?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::generator::ValueClass;

    fn aggregate_slot(offset: i16, size: u32) -> FrameSlot {
        FrameSlot {
            offset,
            class: ValueClass::General,
            size,
            value_type: Type::Struct { size, align: 4 },
            parameter_register: None,
            is_array: false,
        }
    }

    #[test]
    fn places_leading_initializer_lanes_below_contiguous_aggregate_locals() {
        let slots = HashMap::from([
            ("__mwcc_temporary_storage_2".to_owned(), aggregate_slot(8, 8)),
            ("__mwcc_temporary_storage_0".to_owned(), aggregate_slot(16, 8)),
            ("vec2".to_owned(), aggregate_slot(24, 12)),
            ("vec1".to_owned(), aggregate_slot(36, 12)),
            ("tube".to_owned(), aggregate_slot(48, 32)),
        ]);
        let plan = plan(&slots, 0, 48, 8, 2, 0, 88, 88).expect("measured frame");
        assert_eq!(plan.prefix_bytes, 24);
        let offsets = plan
            .slot_moves
            .iter()
            .map(|(name, _, _, offset)| (name.as_str(), *offset))
            .collect::<HashMap<_, _>>();
        assert_eq!(offsets["__mwcc_temporary_storage_0"], 32);
        assert_eq!(offsets["__mwcc_temporary_storage_2"], 40);
        assert_eq!(offsets["vec2"], 48);
        assert_eq!(offsets["vec1"], 60);
        assert_eq!(offsets["tube"], 72);
        assert_eq!((plan.scratch_start, plan.scratch_end), (80, 88));
    }
}
