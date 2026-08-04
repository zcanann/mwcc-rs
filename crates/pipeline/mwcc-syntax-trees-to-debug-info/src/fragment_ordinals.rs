//! Anonymous-symbol timeline for fragmented DWARF containers.
//!
//! GC 4.1 creates the line header in the same translation-unit ordinal stream
//! as function strings, read-only images, pool constants, jump tables, and
//! unwind records. Keep that stateful walk out of the DWARF byte partitioner:
//! this module mirrors the object writer's function-owned payload rules and
//! returns only the two ordinals the fragment container needs.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_machine_code::{MachineFunction, PoolConstant};
use mwcc_versions::CompilerBuild;
use std::collections::HashSet;

pub(super) fn fragment_ordinals(
    machine_functions: &[MachineFunction],
    build: CompilerBuild,
    first_function_counter: u32,
    post_framed_bump: u8,
) -> Compilation<(u32, u32)> {
    let first = machine_functions
        .first()
        .expect("a fragmented function debug unit is nonempty");
    let first_owns_payload = first.owns_anonymous_payload();
    let mut state = OrdinalState::new(first_function_counter);
    let first_number = state.number_before_unwind(first)?;
    let first_ordinal = if first_owns_payload {
        first_number
    } else {
        first_number
            .checked_sub(1)
            .ok_or_else(invalid_fragment_ordinal)?
    };

    let mut close_ordinal = None;
    for (index, machine) in machine_functions.iter().enumerate() {
        let mut number = if index == 0 {
            first_number
        } else {
            state.number_before_unwind(machine)?
        };
        // A payload-free unit creates the early line header in the ordinal
        // immediately preceding the first function block. Once the first
        // function owns a pool object, the header instead follows that payload.
        // If unwind objects follow too, the header consumes the ordinal between
        // the payload and the extab pair (measured on GC 4.1).
        if index == 0 && first_owns_payload && machine.frame.is_some() {
            number = checked_add(number, 1)?;
        }
        if machine.frame.is_some() {
            number = checked_add(number, 2)?;
        }
        if index + 1 == machine_functions.len() {
            close_ordinal = Some(checked_add(
                number,
                u32::from(machine.frame.is_none()),
            )?);
        }
        let post_function_bump = machine.post_function_anonymous_bump.unwrap_or_else(|| {
            if machine.frame.is_some() {
                post_framed_bump
            } else {
                build.post_leaf_function_anonymous_bump
            }
        });
        state.counter = checked_add(number, u32::from(post_function_bump))?
            .checked_sub(machine.post_function_counter_rollback)
            .ok_or_else(invalid_fragment_ordinal)?;
    }
    Ok((
        first_ordinal,
        close_ordinal.expect("a fragmented function debug unit is nonempty"),
    ))
}

pub(super) fn class_fragment_ordinals(
    machine_functions: &[MachineFunction],
    build: CompilerBuild,
    first_function_counter: u32,
    post_framed_bump: u8,
) -> Compilation<(u32, u32)> {
    if build.version != (4, 1, 0) {
        return Err(Diagnostic::error(
            "debug-info: fragmented class ordinals are only measured for GC 4.1",
        ));
    }
    let (ordinary_first, _ordinary_end) = fragment_ordinals(
        machine_functions,
        build,
        first_function_counter,
        post_framed_bump,
    )?;
    let first = checked_add(ordinary_first, 2)
        .map_err(|_| Diagnostic::error("debug-info: invalid GC 4.1 class ordinal"))?;
    let end = checked_add(first, 7)
        .map_err(|_| Diagnostic::error("debug-info: invalid GC 4.1 class ordinal"))?;
    Ok((first, end))
}

pub(super) fn fragmented_post_framed_bump(build: CompilerBuild) -> u8 {
    if build.version == (4, 1, 0) {
        // With `-sym on`, GC 4.1 moves one of the ordinary four framed
        // post-function ordinals into the function's preceding analysis block.
        // Two consecutive framed functions expose a three-ordinal transition.
        3
    } else {
        build.post_framed_function_anonymous_bump
    }
}

struct OrdinalState {
    counter: u32,
    numbered_constants: HashSet<(u64, u8)>,
}

impl OrdinalState {
    fn new(counter: u32) -> Self {
        Self {
            counter,
            numbered_constants: HashSet::new(),
        }
    }

    fn number_before_unwind(&mut self, machine: &MachineFunction) -> Compilation<u32> {
        let owned_statics = u32::try_from(machine.static_locals.len())
            .map_err(|_| invalid_fragment_ordinal())?;
        let unadjusted = checked_add(
            checked_add(self.counter, owned_statics)?,
            machine.object_anonymous_bump(),
        )?;
        let mut number = adjusted_number(unadjusted, machine.constant_number_adjust)?;
        let strings_at_front = machine.string_number_after_constants.is_none()
            && machine.string_number_after_rodata.is_none();
        if strings_at_front {
            number = checked_add(number, machine.new_string_count)?;
        }

        for (blob_index, blob) in machine.anonymous_rodata.iter().enumerate() {
            if let Some((position, gap)) = machine.string_number_after_rodata {
                if position == blob_index as u32 {
                    number = checked_add(number, checked_add(gap, machine.new_string_count)?)?;
                }
            }
            let offset = blob.static_slot_prefix_bump.map_or(blob.anonymous_offset, |prefix| {
                let strings = if strings_at_front {
                    machine.new_string_count
                } else {
                    0
                };
                -1 - i32::try_from(prefix.saturating_add(strings)).unwrap_or(i32::MAX)
            });
            number = checked_add(adjusted_number(number, offset)?, 1)?;
        }
        if let Some((position, gap)) = machine.string_number_after_rodata {
            if position as usize >= machine.anonymous_rodata.len() {
                number = checked_add(number, checked_add(gap, machine.new_string_count)?)?;
            }
        }

        for (constant_index, constant) in machine.constants.iter().enumerate() {
            if constant.static_slot {
                continue;
            }
            if machine.string_number_after_constants == Some(constant_index as u32) {
                number = checked_add(number, machine.new_string_count)?;
            }
            for (_, gap) in machine
                .constant_number_gaps
                .iter()
                .filter(|(index, _)| *index == constant_index)
            {
                number = checked_add(number, *gap)?;
            }
            if consumes_new_ordinal(&mut self.numbered_constants, constant) {
                number = checked_add(number, 1)?;
            }
        }
        if let Some(position) = machine.string_number_after_constants {
            if position as usize >= machine.constants.len() {
                number = checked_add(number, machine.new_string_count)?;
            }
        }
        for table in &machine.jump_tables {
            number = checked_add(number, table.anonymous_offset)?;
        }

        let static_slot_post_bump = machine
            .anonymous_rodata
            .iter()
            .find_map(|blob| blob.static_slot_prefix_bump)
            .map_or(0, |prefix| {
                prefix.saturating_sub(machine.fragmented_debug_static_slot_discount)
                    + if strings_at_front {
                        machine.new_string_count
                    } else {
                        0
                    }
            });
        checked_add(
            number,
            checked_add(
                machine.post_constant_label_bump,
                static_slot_post_bump,
            )?,
        )
        .and_then(|number| checked_add(number, machine.fragmented_debug_anonymous_bump))
    }
}

fn consumes_new_ordinal(
    numbered: &mut HashSet<(u64, u8)>,
    constant: &PoolConstant,
) -> bool {
    constant.force_new || numbered.insert((constant.bits, constant.byte_width))
}

fn adjusted_number(base: u32, adjustment: i32) -> Compilation<u32> {
    u32::try_from(i64::from(base) + i64::from(adjustment))
        .map_err(|_| invalid_fragment_ordinal())
}

fn checked_add(left: u32, right: u32) -> Compilation<u32> {
    left.checked_add(right)
        .ok_or_else(invalid_fragment_ordinal)
}

fn invalid_fragment_ordinal() -> Diagnostic {
    Diagnostic::error("debug-info: invalid GC 4.1 fragment ordinal")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{AnonymousRodata, FrameInfo};
    use mwcc_versions::GC_3_0A3;

    fn constant(bits: u64) -> PoolConstant {
        PoolConstant {
            bits,
            byte_width: 4,
            static_slot: false,
            image: false,
            force_new: false,
        }
    }

    #[test]
    fn pool_free_leaf_keeps_the_header_before_the_function_block() {
        let mut function = MachineFunction::new("leaf");
        function.anonymous_label_bump = 2;

        assert_eq!(
            fragment_ordinals(&[function], GC_3_0A3, 5, 3).unwrap(),
            (6, 8)
        );
    }

    #[test]
    fn first_leaf_constant_places_the_header_after_the_pool() {
        let mut function = MachineFunction::new("scale");
        function.constants.push(constant(0x4050_0000));

        assert_eq!(
            fragment_ordinals(&[function], GC_3_0A3, 6, 3).unwrap(),
            (7, 8)
        );
    }

    #[test]
    fn conversion_gap_restores_only_the_untraversed_image_prefix() {
        let mut function = MachineFunction::new("scale");
        function.has_conversion = true;
        function.anonymous_label_bump = 3;
        function.constants.push(PoolConstant {
            bits: 0x4330_0000_8000_0000,
            byte_width: 8,
            static_slot: false,
            image: false,
            force_new: false,
        });
        function.constant_number_gaps.push((0, 2));
        function.anonymous_rodata.push(AnonymousRodata {
            bytes: vec![0; 16],
            static_slot_prefix_bump: Some(3),
            anonymous_offset: 0,
        });
        function.fragmented_debug_static_slot_discount = 2;

        assert_eq!(
            fragment_ordinals(&[function], GC_3_0A3, 7, 3).unwrap(),
            (12, 13)
        );
    }

    #[test]
    fn first_framed_payload_places_the_header_before_extab() {
        let mut function = MachineFunction::new("scale_and_sink");
        function.constants = vec![constant(0x4050_0000), constant(0x3f80_0000)];
        function.frame = Some(FrameInfo::default());

        assert_eq!(
            fragment_ordinals(&[function], GC_3_0A3, 7, 3).unwrap(),
            (9, 12)
        );
    }

    #[test]
    fn constants_dedupe_across_function_blocks() {
        let mut first = MachineFunction::new("first");
        first.constants.push(constant(0x4050_0000));
        let mut second = MachineFunction::new("second");
        second.constants.push(constant(0x4050_0000));

        assert_eq!(
            fragment_ordinals(&[first, second], GC_3_0A3, 6, 3).unwrap(),
            (7, 12)
        );
    }
}
