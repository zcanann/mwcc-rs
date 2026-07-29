//! Linkage-first scheduling for frame receivers with global float arguments.
//!
//! Build 163 issues the independent global-address high half before loading a
//! frame-backed call receiver. The low address operation and float load remain
//! after the receiver. This owner recognizes both direct and materialized-low
//! address packets before applying that permutation.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_linkage_first_global_float_arguments(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }

        let mut start = 0;
        while start < self.output.instructions.len() {
            let Some(packet_len) =
                global_float_argument_packet_len(&self.output.instructions[start..])
            else {
                start += 1;
                continue;
            };
            self.move_instruction_before(start + 1, start);
            start += packet_len;
        }
    }
}

fn global_float_argument_packet_len(instructions: &[Instruction]) -> Option<usize> {
    if matches!(
        instructions,
        [
            Instruction::LoadWord { d: 3, a: 1, .. },
            Instruction::AddImmediateShifted { d: 4, a: 0, .. },
            Instruction::LoadFloatSingle { d: 1, a: 4, .. },
            Instruction::BranchAndLink { .. },
            ..
        ]
    ) {
        return Some(4);
    }
    if matches!(
        instructions,
        [
            Instruction::LoadWord { d: 3, a: 1, .. },
            Instruction::AddImmediateShifted { d: 4, a: 0, .. },
            Instruction::AddImmediate { d: 4, a: 4, .. },
            Instruction::LoadFloatSingle { d: 1, a: 4, .. },
            Instruction::BranchAndLink { .. },
            ..
        ]
    ) {
        return Some(5);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_direct_and_materialized_low_global_float_packets() {
        let receiver = Instruction::LoadWord {
            d: 3,
            a: 1,
            offset: 28,
        };
        let high = Instruction::AddImmediateShifted {
            d: 4,
            a: 0,
            immediate: 0,
        };
        let low = Instruction::AddImmediate {
            d: 4,
            a: 4,
            immediate: 0,
        };
        let load = Instruction::LoadFloatSingle {
            d: 1,
            a: 4,
            offset: 0,
        };
        let call = Instruction::BranchAndLink {
            target: "animate".into(),
        };

        assert_eq!(
            global_float_argument_packet_len(&[
                receiver.clone(),
                high.clone(),
                load.clone(),
                call.clone()
            ]),
            Some(4)
        );
        assert_eq!(
            global_float_argument_packet_len(&[receiver, high, low, load, call]),
            Some(5)
        );
    }
}
