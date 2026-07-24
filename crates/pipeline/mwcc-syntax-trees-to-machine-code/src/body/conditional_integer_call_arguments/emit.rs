//! Build-163 schedule for conditional integer wrapper calls.

#[allow(unused_imports)]
use super::super::*;
use super::recognize::recognize;

fn push_conditional(generator: &mut Generator) -> usize {
    let index = generator.output.instructions.len();
    generator
        .output
        .instructions
        .push(Instruction::BranchConditionalForward {
            options: 12,
            condition_bit: 2,
            target: 0,
        });
    index
}

fn push_branch(generator: &mut Generator) -> usize {
    let index = generator.output.instructions.len();
    generator
        .output
        .instructions
        .push(Instruction::Branch { target: 0 });
    index
}

fn patch(generator: &mut Generator, index: usize, target_index: usize) {
    match &mut generator.output.instructions[index] {
        Instruction::BranchConditionalForward { target, .. } | Instruction::Branch { target } => {
            *target = target_index
        }
        _ => unreachable!("conditional integer call patch point is a branch"),
    }
}

impl Generator {
    pub(crate) fn try_conditional_integer_call_arguments(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = recognize(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self.behavior.integer_select_style
                != mwcc_versions::IntegerSelectStyle::BranchPreserving
            || function
                .parameters
                .iter()
                .enumerate()
                .any(|(index, parameter)| {
                    self.locations
                        .get(&parameter.name)
                        .map(|location| location.register)
                        != u8::try_from(index + 3).ok()
                })
        {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        self.non_leaf = true;
        self.frame_size = 32;
        self.callee_saved.clear();
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 6,
                clear: 24,
            });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            });
        let local_false = push_conditional(self);
        self.output
            .instructions
            .push(Instruction::load_immediate(6, shape.local_true));
        let local_join = push_branch(self);
        let local_false_target = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::load_immediate(6, 0));
        let local_join_target = self.output.instructions.len();
        patch(self, local_false, local_false_target);
        patch(self, local_join, local_join_target);

        self.output
            .instructions
            .push(Instruction::ClearLeftImmediateRecord {
                a: 0,
                s: 5,
                clear: 24,
            });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 8,
            a: 6,
            immediate: 0,
        });
        let argument_false = push_conditional(self);
        self.output
            .instructions
            .push(Instruction::load_immediate(6, shape.argument_true));
        let argument_join = push_branch(self);
        let argument_false_target = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::load_immediate(6, 0));
        let argument_join_target = self.output.instructions.len();
        patch(self, argument_false, argument_false_target);
        patch(self, argument_join, argument_join_target);

        self.output.instructions.extend([
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 8,
            },
            Instruction::AddImmediate {
                d: 9,
                a: 8,
                immediate: 0,
            },
            Instruction::load_immediate(5, 0),
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 12,
            },
            Instruction::load_immediate(10, 0),
        ]);
        self.record_relocation(RelocationKind::Rel24, shape.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.callee.to_string(),
        });
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 36,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 32,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}
