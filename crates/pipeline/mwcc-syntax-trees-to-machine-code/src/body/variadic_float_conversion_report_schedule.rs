//! Final schedule for paired float-to-unsigned variadic report arguments.
//!
//! Expression lowering owns the right-to-left helper-call order and retained
//! result lifetime. Once registers are physical, build 163 places `crclr`
//! immediately after the second conversion and uses canonical `mr` encodings
//! for the retained result and saved-index arguments.

#[allow(unused_imports)]
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FloatConversionReportPlan {
    start: usize,
    retained_result: u8,
    index: u8,
}

impl Generator {
    pub(crate) fn schedule_variadic_float_conversion_reports(&mut self) {
        while let Some(plan) = float_conversion_report_plan(&self.output.instructions) {
            basic_block_schedule::permute_contents(
                &mut self.output,
                plan.start,
                [0, 1, 2, 3, 4, 9, 5, 6, 7, 8, 10],
            );
            self.output.instructions[plan.start + 1] =
                Instruction::move_register(plan.retained_result, 3);
            self.output.instructions[plan.start + 6] =
                Instruction::move_register(4, plan.index);
            self.output.instructions[plan.start + 7] =
                Instruction::move_register(7, plan.retained_result);
        }
    }
}

fn float_conversion_report_plan(
    instructions: &[Instruction],
) -> Option<FloatConversionReportPlan> {
    instructions.windows(11).enumerate().find_map(|(start, window)| {
        let [
            Instruction::BranchAndLink { target: first_conversion },
            Instruction::AddImmediate {
                d: retained_result,
                a: 3,
                immediate: 0,
            },
            Instruction::LoadFloatDouble {
                d: 1,
                a: value_base,
                ..
            },
            Instruction::BranchAndLink { target: second_conversion },
            Instruction::Or { a: 5, s: 3, b: 3 },
            Instruction::AddImmediate {
                d: 4,
                a: first_index,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 7,
                a: retained_argument,
                immediate: 0,
            },
            Instruction::AddImmediate {
                d: 3,
                a: format_base,
                ..
            },
            Instruction::AddImmediate {
                d: 6,
                a: second_index,
                immediate: 1,
            },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink { target: report },
        ] = window
        else {
            return None;
        };
        (first_conversion == "__cvt_fp2unsigned"
            && second_conversion == first_conversion
            && report == "OSReport"
            && (14..=31).contains(retained_result)
            && retained_argument == retained_result
            && (14..=31).contains(first_index)
            && first_index == second_index
            && (14..=31).contains(value_base)
            && (14..=31).contains(format_base))
            .then_some(FloatConversionReportPlan {
                start,
                retained_result: *retained_result,
                index: *first_index,
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_paired_float_conversion_report_arguments() {
        let instructions = [
            Instruction::BranchAndLink { target: "__cvt_fp2unsigned".into() },
            Instruction::AddImmediate { d: 27, a: 3, immediate: 0 },
            Instruction::LoadFloatDouble { d: 1, a: 26, offset: 144 },
            Instruction::BranchAndLink { target: "__cvt_fp2unsigned".into() },
            Instruction::move_register(5, 3),
            Instruction::AddImmediate { d: 4, a: 25, immediate: 0 },
            Instruction::AddImmediate { d: 7, a: 27, immediate: 0 },
            Instruction::AddImmediate { d: 3, a: 31, immediate: 288 },
            Instruction::AddImmediate { d: 6, a: 25, immediate: 1 },
            Instruction::ConditionRegisterClear { d: 6 },
            Instruction::BranchAndLink { target: "OSReport".into() },
        ];
        assert_eq!(
            float_conversion_report_plan(&instructions),
            Some(FloatConversionReportPlan {
                start: 0,
                retained_result: 27,
                index: 25,
            })
        );
    }
}
