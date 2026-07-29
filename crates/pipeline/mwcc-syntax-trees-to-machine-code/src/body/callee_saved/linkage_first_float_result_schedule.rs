//! Linkage-first scheduling across adjacent calls with a saved float result.
//!
//! Build 163 fills the float-result copy latency with the next call's
//! independent frame-backed receiver load. Selection and allocation emit the
//! two operations in dependency order; this owner recognizes the complete
//! physical packet before exchanging them.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn schedule_linkage_first_float_result_latency(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst {
            return;
        }

        let mut start = 0;
        while start + 3 < self.output.instructions.len() {
            if is_float_result_latency_packet(&self.output.instructions[start..start + 4]) {
                self.move_instruction_before(start + 2, start + 1);
                start += 4;
            } else {
                start += 1;
            }
        }
    }
}

fn is_float_result_latency_packet(instructions: &[Instruction]) -> bool {
    matches!(
        instructions,
        [
            Instruction::BranchAndLink { .. },
            Instruction::FloatMove {
                d: saved,
                b: 1
            },
            Instruction::LoadWord { d: 3, a: 1, .. },
            Instruction::BranchAndLink { .. },
        ] if *saved >= 14
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_only_a_saved_float_result_and_frame_receiver_packet() {
        let instructions = vec![
            Instruction::BranchAndLink {
                target: "first".into(),
            },
            Instruction::FloatMove { d: 31, b: 1 },
            Instruction::LoadWord {
                d: 3,
                a: 1,
                offset: 20,
            },
            Instruction::BranchAndLink {
                target: "second".into(),
            },
        ];

        assert!(is_float_result_latency_packet(&instructions));
    }
}
