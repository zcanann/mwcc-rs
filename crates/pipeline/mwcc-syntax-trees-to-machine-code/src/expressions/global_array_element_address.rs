//! Address formation for global-array elements with computed indices.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Materialize `&global[computed_index]` when the index is not already a
    /// register leaf. MWCC finishes consuming the index before reusing r0 for
    /// the array's low relocated address, so the two values never need to be
    /// live in separate allocator-owned homes.
    pub(crate) fn try_emit_computed_global_array_element_address(
        &mut self,
        name: &str,
        total_size: u32,
        element_size: u32,
        index: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        if destination == GENERAL_SCRATCH
            || constant_value(index).is_some()
            || self.general_register_of_leaf(index).is_ok()
        {
            return Ok(false);
        }

        self.evaluate_general(index, GENERAL_SCRATCH)?;
        let scaled = self.fresh_virtual_general_preferring(4);
        if element_size.is_power_of_two() {
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: scaled,
                    s: GENERAL_SCRATCH,
                    shift: element_size.trailing_zeros() as u8,
                });
        } else {
            let immediate = i16::try_from(element_size).map_err(|_| {
                Diagnostic::error("global-array element size is too large to scale (roadmap)")
            })?;
            self.output
                .instructions
                .push(Instruction::MultiplyImmediate {
                    d: scaled,
                    a: GENERAL_SCRATCH,
                    immediate,
                });
        }

        let small =
            self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8;
        let base = if small {
            self.emit_global_array_base(name, total_size, destination)?;
            destination
        } else {
            let high = self.fresh_virtual_general_preferring(3);
            self.emit_address_high(high, name);
            self.record_relocation(RelocationKind::Addr16Lo, name);
            self.output.instructions.push(Instruction::AddImmediate {
                d: GENERAL_SCRATCH,
                a: high,
                immediate: 0,
            });
            GENERAL_SCRATCH
        };
        self.output.instructions.push(Instruction::Add {
            d: destination,
            a: base,
            b: scaled,
        });
        Ok(true)
    }
}
