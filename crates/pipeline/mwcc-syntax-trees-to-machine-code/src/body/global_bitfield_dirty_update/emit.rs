//! Build-163 schedule for a global field insertion and dirty-mask update.

#[allow(unused_imports)]
use super::super::*;
use super::recognize::recognize;

impl Generator {
    pub(crate) fn try_global_bitfield_dirty_update(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(shape) = recognize(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || self
                .locations
                .get(shape.parameter)
                .map(|location| location.register)
                != Some(3)
        {
            return Ok(false);
        }
        let Some(&global_type) = self.globals.get(shape.global) else {
            return Ok(false);
        };
        self.output.pre_scheduled = true;
        self.evaluate(&Expression::Variable(shape.global.into()), global_type, 4)?;
        self.output.instructions.extend([
            Instruction::RotateAndMask {
                a: 0,
                s: 3,
                shift: 16,
                begin: 8,
                end: 15,
            },
            Instruction::LoadWordWithUpdate {
                d: 3,
                a: 4,
                offset: shape.field_offset,
            },
            Instruction::RotateAndMask {
                a: 3,
                s: 3,
                shift: 0,
                begin: 16,
                end: 12,
            },
            Instruction::Or { a: 0, s: 3, b: 0 },
            Instruction::StoreWord {
                s: 0,
                a: 4,
                offset: 0,
            },
        ]);
        self.evaluate(&Expression::Variable(shape.global.into()), global_type, 3)?;
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: shape.dirty_offset,
            },
            Instruction::OrImmediate {
                a: 0,
                s: 0,
                immediate: shape.dirty_mask,
            },
            Instruction::StoreWord {
                s: 0,
                a: 3,
                offset: shape.dirty_offset,
            },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}
