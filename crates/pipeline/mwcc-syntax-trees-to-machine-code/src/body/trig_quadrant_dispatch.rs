//! Generation-specific quadrant selection for fdlibm trig dispatchers.

use super::*;
use mwcc_versions::TrigQuadrantDispatchStyle;

pub(super) struct TrigQuadrant {
    pub(super) callee: String,
    pub(super) int_argument: Option<i16>,
    pub(super) negated: bool,
}

impl Generator {
    pub(super) fn emit_trig_quadrant_dispatch(
        &mut self,
        quadrants: [&TrigQuadrant; 4],
        epilogue: mwcc_vreg::Label,
    ) -> u32 {
        let case0 = self.fresh_label();
        let case1 = self.fresh_label();
        let case2 = self.fresh_label();
        let case3 = self.fresh_label();

        let hidden_label_bump = match self.behavior.trig_quadrant_dispatch_style {
            TrigQuadrantDispatchStyle::BinarySearch => {
                self.output.instructions.push(Instruction::RotateAndMask {
                    a: 0,
                    s: 3,
                    shift: 0,
                    begin: 30,
                    end: 31,
                });
                let mid = self.fresh_label();
                self.output
                    .instructions
                    .push(Instruction::CompareWordImmediate { a: 0, immediate: 1 });
                self.emit_branch_conditional_to(12, 2, case1); // beq
                self.emit_branch_conditional_to(4, 0, mid); // bge -> the 2/3 side
                self.output
                    .instructions
                    .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
                self.emit_branch_conditional_to(4, 0, case0); // bge
                self.emit_branch_to(case3);
                self.bind_label(mid);
                self.output
                    .instructions
                    .push(Instruction::CompareWordImmediate { a: 0, immediate: 3 });
                self.emit_branch_conditional_to(4, 0, case3); // bge
                self.emit_branch_to(case2);
                0
            }
            TrigQuadrantDispatchStyle::LinearChain => {
                self.output
                    .instructions
                    .push(Instruction::RotateAndMaskRecord {
                        a: 0,
                        s: 3,
                        shift: 0,
                        begin: 30,
                        end: 31,
                    });
                self.emit_branch_conditional_to(12, 2, case0); // beq
                for (value, label) in [(1, case1), (2, case2)] {
                    self.output
                        .instructions
                        .push(Instruction::CompareWordImmediate {
                            a: 0,
                            immediate: value,
                        });
                    self.emit_branch_conditional_to(12, 2, label); // beq
                }
                self.emit_branch_to(case3);
                // Build 145 contracts three emitted branch-tree nodes but
                // retains nine more internal CFG labels (pool @35 versus @26
                // for the otherwise identical 4.1 dispatcher).
                9
            }
        };

        let emit_arm = |generator: &mut Self,
                        quadrant: &TrigQuadrant,
                        label,
                        falls: bool| {
            generator.bind_label(label);
            generator
                .output
                .instructions
                .push(Instruction::LoadFloatDouble {
                    d: 1,
                    a: 1,
                    offset: 16,
                });
            if let Some(int_argument) = quadrant.int_argument {
                generator
                    .output
                    .instructions
                    .push(Instruction::load_immediate(3, int_argument));
            }
            generator
                .output
                .instructions
                .push(Instruction::LoadFloatDouble {
                    d: 2,
                    a: 1,
                    offset: 24,
                });
            generator.record_relocation(RelocationKind::Rel24, &quadrant.callee);
            generator
                .output
                .instructions
                .push(Instruction::BranchAndLink {
                    target: quadrant.callee.clone(),
                });
            if quadrant.negated {
                generator
                    .output
                    .instructions
                    .push(Instruction::FloatNegate { d: 1, b: 1 });
            }
            if !falls {
                generator.emit_branch_to(epilogue);
            }
        };
        let [q0, q1, q2, q3] = quadrants;
        emit_arm(self, q0, case0, false);
        emit_arm(self, q1, case1, false);
        emit_arm(self, q2, case2, false);
        emit_arm(self, q3, case3, true);
        hidden_label_bump
    }
}
