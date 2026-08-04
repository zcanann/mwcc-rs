//! Packed-byte and floating-conversion tail for dense counted loops.
//!
//! MWCC overlaps three independent transactions: ADPCM nibble packing, table
//! index formation, and signed-integer conversion through the stack bias. The
//! generic scheduler keeps each transaction contiguous and pins intermediate
//! expressions to r0. This owner proves the complete packet, reuses values only
//! after their final reads, and applies the measured latency-covering order
//! before physical allocation.

#[allow(unused_imports)]
use super::*;

const TAIL_LEN: usize = 29;
const TAIL_ORDER: [usize; TAIL_LEN] = [
    21, 3, 22, 0, 4, 17, 23, 18, 19, 9, 24, 6, 20, 10, 25, 11, 5, 7, 8, 1, 26,
    2, 12, 13, 14, 15, 27, 16, 28,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TailPlan {
    start: usize,
    index: u8,
    n_dl: u8,
    n_l3: u8,
    n_l2: u8,
    n_l1: u8,
    n_l0: u8,
    parity: u8,
    scale_l2: u8,
    table_index: u8,
}

impl Generator {
    pub(crate) fn schedule_dense_counted_loop_tail(&mut self) -> bool {
        if !self.structured_dense_counted_loop_entry_owner {
            return false;
        }
        let Some(plan) = locate_tail(&self.output.instructions) else {
            return false;
        };
        self.prefer_virtual_general(plan.parity, 22);
        self.prefer_virtual_general(plan.scale_l2, 19);
        self.prefer_virtual_general(plan.table_index, 8);
        self.prefer_virtual_general(plan.n_l3, 7);
        self.prefer_virtual_general(plan.n_l2, 8);
        self.prefer_virtual_general(plan.n_l1, 9);
        self.prefer_virtual_general(plan.n_l0, 10);

        let mut old = self.output.instructions[plan.start..plan.start + TAIL_LEN].to_vec();
        // nL1's scaled form replaces the now-dead boolean in r9 and then
        // accumulates nL2*4 for the packed output byte.
        old[4] = Instruction::ShiftLeftImmediate {
            a: plan.n_l1,
            s: plan.n_l1,
            shift: 1,
        };
        old[6] = Instruction::Add {
            d: plan.n_l1,
            a: plan.n_l1,
            b: plan.scale_l2,
        };
        // The table index and signed ii/2 conversion share r8/r12 in disjoint
        // phases. The conversion consumes nDL's old integer bits first.
        old[17] = Instruction::Add {
            d: plan.table_index,
            a: plan.n_l0,
            b: plan.scale_l2,
        };
        old[18] = Instruction::Add {
            d: plan.table_index,
            a: plan.n_l1,
            b: plan.table_index,
        };
        old[19] = Instruction::ShiftLeftImmediate {
            a: plan.table_index,
            s: plan.table_index,
            shift: 3,
        };
        old[9] = Instruction::ShiftRightLogicalImmediate {
            a: plan.n_dl,
            s: plan.index,
            shift: 31,
        };
        old[10] = Instruction::Add {
            d: plan.table_index,
            a: plan.n_dl,
            b: plan.index,
        };
        old[11] = Instruction::ShiftRightAlgebraicImmediate {
            a: plan.n_dl,
            s: plan.table_index,
            shift: 1,
        };

        // Reuse the dead nL3 and table-index homes for the byte merge. The
        // parity mask replaces nL1 only after both scaled sums have consumed it.
        old[5] = Instruction::ShiftLeftImmediate {
            a: plan.table_index,
            s: plan.n_l3,
            shift: 3,
        };
        old[7] = Instruction::Add {
            d: plan.n_l3,
            a: plan.n_l0,
            b: plan.n_l1,
        };
        old[8] = Instruction::Add {
            d: plan.n_l3,
            a: plan.table_index,
            b: plan.n_l3,
        };
        old[2] = Instruction::RotateAndMask {
            a: plan.n_l1,
            s: plan.parity,
            shift: 0,
            begin: 29,
            end: 29,
        };
        old[12] = Instruction::ClearLeftImmediate {
            a: plan.n_l3,
            s: plan.n_l3,
            clear: 24,
        };
        let (byte_base, _) = match old[13] {
            Instruction::LoadByteZeroIndexed { a, b, .. } => (a, b),
            _ => unreachable!("the packed tail byte load was recognized"),
        };
        old[13] = Instruction::LoadByteZeroIndexed {
            d: plan.table_index,
            a: byte_base,
            b: plan.n_dl,
        };
        old[14] = Instruction::ShiftLeftWord {
            a: plan.n_l3,
            s: plan.n_l3,
            b: plan.n_l1,
        };
        old[15] = Instruction::Or {
            a: plan.n_l3,
            s: plan.table_index,
            b: plan.n_l3,
        };
        old[16] = Instruction::StoreByteIndexed {
            s: plan.n_l3,
            a: byte_base,
            b: plan.n_dl,
        };

        // The table value is f0; f1 carries the biased integer until the
        // multiply consumes both. This spelling matches MWCC's conversion
        // pipeline and avoids a late float copy.
        let table_base = match old[20] {
            Instruction::LoadFloatDoubleIndexed { a, .. } => a,
            _ => unreachable!("the counted tail table load was recognized"),
        };
        old[20] = Instruction::LoadFloatDoubleIndexed {
            d: 0,
            a: table_base,
            b: plan.table_index,
        };
        old[24] = Instruction::LoadFloatDouble {
            d: 1,
            a: 1,
            offset: 72,
        };
        old[25] = Instruction::FloatSubtractDouble { d: 1, a: 1, b: 2 };
        old[26] = Instruction::FloatMultiplyDouble {
            d: 0,
            a: 1,
            c: 0,
        };

        let mut permutation: Vec<usize> = (0..self.output.instructions.len()).collect();
        for (new_relative, old_relative) in TAIL_ORDER.into_iter().enumerate() {
            self.output.instructions[plan.start + new_relative] = old[old_relative].clone();
            permutation[plan.start + old_relative] = plan.start + new_relative;
        }
        crate::remap_instruction_indices(self, &permutation);
        // The clamp join formerly entered at the parity calculation. Everything
        // hoisted ahead of it is loop-tail work on both clamp paths.
        crate::retarget_instruction_destinations(self, plan.start + 3, plan.start);
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
        true
    }

    pub(crate) fn schedule_dense_counted_loop_epilogue(&mut self) -> bool {
        if !self.structured_dense_counted_loop_entry_owner {
            return false;
        }
        let Some(start) = self.output.instructions.windows(9).position(|window| {
            let mut base = None;
            for (index, instruction) in window[..6].iter().enumerate() {
                let Instruction::StoreWord { a, offset, .. } = instruction else {
                    return false;
                };
                if *offset != (index * 4) as i16 {
                    return false;
                }
                if let Some(base) = base {
                    if *a != base {
                        return false;
                    }
                } else {
                    base = Some(*a);
                }
            }
            matches!(window[6], Instruction::Or { a: 3, s, b } if s == b)
                && matches!(window[7], Instruction::AddImmediate { d: 11, a: 1, immediate } if immediate > 0)
                && matches!(&window[8], Instruction::BranchAndLink { target } if target == "_restgpr_19")
        }) else {
            return false;
        };
        let order = [0, 7, 6, 1, 2, 3, 4, 5];
        let old = self.output.instructions[start..start + 8].to_vec();
        let mut permutation: Vec<usize> = (0..self.output.instructions.len()).collect();
        for (new_relative, old_relative) in order.into_iter().enumerate() {
            self.output.instructions[start + new_relative] = old[old_relative].clone();
            permutation[start + old_relative] = start + new_relative;
        }
        crate::remap_instruction_indices(self, &permutation);
        self.output
            .relocations
            .sort_by_key(|relocation| relocation.instruction_index);
        true
    }
}

fn locate_tail(instructions: &[Instruction]) -> Option<TailPlan> {
    instructions
        .windows(TAIL_LEN)
        .enumerate()
        .find_map(|(start, window)| recognize_tail(window).map(|plan| TailPlan { start, ..plan }))
}

fn recognize_tail(window: &[Instruction]) -> Option<TailPlan> {
    let Instruction::RotateAndMask {
        a: parity,
        s: index,
        shift: 0,
        begin: 31,
        end: 31,
    } = window[0]
    else {
        return None;
    };
    let Instruction::RotateAndMask {
        a: parity_mask,
        s: parity_source,
        shift: 0,
        begin: 29,
        end: 29,
    } = window[2]
    else {
        return None;
    };
    let Instruction::ShiftLeftImmediate {
        a: scale_l2,
        s: n_l2,
        shift: 2,
    } = window[3]
    else {
        return None;
    };
    let Instruction::ShiftLeftImmediate {
        a: scale_l1,
        s: n_l1,
        shift: 1,
    } = window[4]
    else {
        return None;
    };
    let Instruction::ShiftLeftImmediate {
        a: 0,
        s: n_l3,
        shift: 3,
    } = window[5]
    else {
        return None;
    };
    let Instruction::Add {
        d: packed,
        a: n_l0,
        b: 0,
    } = window[8]
    else {
        return None;
    };
    let Instruction::ShiftRightLogicalImmediate {
        a: offset_sign,
        s: offset_index,
        shift: 31,
    } = window[9]
    else {
        return None;
    };
    let Instruction::ShiftRightAlgebraicImmediate {
        a: byte_offset,
        s: offset_sum,
        shift: 1,
    } = window[11]
    else {
        return None;
    };
    let Instruction::Add {
        d: table_index,
        a: table_first,
        b: table_second,
    } = window[17]
    else {
        return None;
    };
    let Instruction::XorImmediateShifted {
        a: n_dl,
        s: n_dl_source,
        immediate: 32768,
    } = window[21]
    else {
        return None;
    };
    macro_rules! require {
        ($name:literal, $condition:expr) => {
            if !$condition {
                if std::env::var_os("MWCC_CAPTURE_FUNCTION").is_some() {
                    eprintln!("dense tail rejected at {}", $name);
                }
                return None;
            }
        };
    }
    require!("parity source", parity_source == parity);
    require!("offset index", offset_index == index);
    require!("nDL source", n_dl_source == n_dl);
    require!("table first", table_first == scale_l1);
    require!("table second", table_second == n_l0);
    require!("parity decrement", matches!(window[1], Instruction::AddImmediate { d, a, immediate: -1 } if d == parity && a == parity));
    require!("packed l2 add", matches!(window[6], Instruction::Add { d: 0, a, b: 0 } if a == scale_l2));
    require!("packed l1 add", matches!(window[7], Instruction::Add { d: 0, a, b: 0 } if a == scale_l1));
    require!("offset add", matches!(window[10], Instruction::Add { d, a, b } if d == offset_sum && a == offset_sign && b == index));
    require!("packed clear", matches!(window[12], Instruction::ClearLeftImmediate { a: 0, s, clear: 24 } if s == packed));
    require!("byte load", matches!(window[13], Instruction::LoadByteZeroIndexed { b, .. } if b == byte_offset));
    require!("packed shift", matches!(window[14], Instruction::ShiftLeftWord { a: 0, s: 0, b } if b == parity_mask));
    require!("packed or", matches!(window[15], Instruction::Or { a: 0, b: 0, .. }));
    require!("byte store", matches!(window[16], Instruction::StoreByteIndexed { s: 0, b, .. } if b == byte_offset));
    require!("table second add", matches!(window[18], Instruction::Add { d, a, b } if d == table_index && a == scale_l2 && b == table_index));
    require!("table scale", matches!(window[19], Instruction::ShiftLeftImmediate { a, s, shift: 3 } if s == table_index && a != 0));
    require!("table load", matches!(window[20], Instruction::LoadFloatDoubleIndexed { b, .. } if matches!(window[19], Instruction::ShiftLeftImmediate { a, .. } if a == b)));
    require!("conversion word", matches!(window[22], Instruction::StoreWord { s, a: 1, offset: 76 } if s == n_dl));
    require!("bias word", matches!(window[23], Instruction::StoreWord { a: 1, offset: 72, .. }));
    require!("conversion load", matches!(window[24], Instruction::LoadFloatDouble { d: 0, a: 1, offset: 72 }));
    require!("conversion subtract", matches!(window[25], Instruction::FloatSubtractDouble { d: 0, a: 0, b: 2 }));
    require!("table multiply", matches!(window[26], Instruction::FloatMultiplyDouble { d: 0, a: 0, .. }));
    require!("integer conversion", matches!(window[27], Instruction::ConvertToIntegerWordZero { d: 0, b: 0 }));
    require!("integer store", matches!(window[28], Instruction::StoreFloatDouble { s: 0, a: 1, offset: 80 }));
    Some(TailPlan {
        start: 0,
        index,
        n_dl,
        n_l3,
        n_l2,
        n_l1,
        n_l0,
        parity,
        scale_l2,
        table_index,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn measured_tail_order_covers_conversion_latency_before_byte_merge() {
        assert_eq!(&TAIL_ORDER[..8], &[21, 3, 22, 0, 4, 17, 23, 18]);
        assert_eq!(&TAIL_ORDER[20..], &[26, 2, 12, 13, 14, 15, 27, 16, 28]);
    }
}
