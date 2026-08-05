//! Pass-through calls to a function pointer stored in a global record.
//!
//! When every argument already occupies its ABI lane, MWCC uses the first free
//! lane for the global-record address. This avoids extending the r12 callee
//! scratch's live range across address materialization and leaves the incoming
//! arguments untouched.

use super::*;

impl Generator {
    /// Materialize the address and callee for a pass-through call through a
    /// member of a global record.  Both linked calls and terminal sibling
    /// transfers use the first unoccupied argument lane for the record base;
    /// r12 remains exclusively the indirect callee.
    pub(in crate::body) fn try_prepare_global_member_forward_indirect_call(
        &mut self,
        target: &Expression,
        arguments: &[Expression],
    ) -> Compilation<bool> {
        let Some((global, offset)) = global_member_forward_call(target, arguments) else {
            return Ok(false);
        };
        if (!self.addressable_globals.contains_key(global) && !self.globals.contains_key(global))
            || arguments.len() > 8
        {
            return Ok(false);
        }

        for (index, argument) in arguments.iter().enumerate() {
            let (source, width, _) = self.leaf_info(argument)?;
            if width != 32 || source != Eabi::FIRST_GENERAL_ARGUMENT + index as u8 {
                return Ok(false);
            }
        }

        let base = Eabi::FIRST_GENERAL_ARGUMENT + arguments.len() as u8;
        if base >= 12 {
            return Ok(false);
        }
        let offset = i16::try_from(offset)
            .map_err(|_| Diagnostic::error("global indirect-call member offset is out of range"))?;

        self.emit_address_high(base, global);
        self.record_relocation(RelocationKind::Addr16Lo, global);
        self.output.instructions.push(Instruction::AddImmediate {
            d: base,
            a: base,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: base,
            offset,
        });
        Ok(true)
    }

    pub(super) fn try_emit_global_member_forward_indirect_call(
        &mut self,
        target: &Expression,
        arguments: &[Expression],
    ) -> Compilation<bool> {
        if !self.try_prepare_global_member_forward_indirect_call(target, arguments)? {
            return Ok(false);
        }
        self.emit_indirect_branch_and_link(12);
        Ok(true)
    }

    pub(crate) fn schedule_linkage_first_global_member_forward(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !is_global_member_forward_linkage_entry(&self.output)
        {
            return;
        }

        crate::move_instruction_before_retargeting(self, 2, 1);
        crate::move_instruction_before_retargeting(self, 4, 2);
    }
}

fn global_member_forward_call<'a>(
    target: &'a Expression,
    _arguments: &[Expression],
) -> Option<(&'a str, u32)> {
    let Expression::Member {
        base,
        offset,
        index_stride: None,
        ..
    } = target
    else {
        return None;
    };
    let Expression::Variable(global) = base.as_ref() else {
        return None;
    };
    Some((global, *offset))
}

fn is_global_member_forward_linkage_entry(
    output: &mwcc_machine_code::MachineFunction,
) -> bool {
    let [
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::AddImmediateShifted { d: base, a: 0, .. },
        Instruction::StoreWord { s: 0, a: 1, offset: 4 },
        Instruction::AddImmediate { d: low_d, a: low_a, immediate: 0 },
        Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -8 },
        Instruction::LoadWord { d: 12, a: load_base, .. },
        Instruction::MoveToLinkRegister { s: 12 },
        Instruction::BranchToLinkRegisterAndLink,
        ..
    ] = output.instructions.as_slice()
    else {
        return false;
    };
    if base != low_d || base != low_a || base != load_base || !(3..12).contains(base) {
        return false;
    }
    let relocation = |index, kind| {
        output.relocations.iter().find(|relocation| {
            relocation.instruction_index == index && relocation.kind == kind
        })
    };
    let (Some(high), Some(low)) = (
        relocation(1, RelocationKind::Addr16Ha),
        relocation(3, RelocationKind::Addr16Lo),
    ) else {
        return false;
    };
    matches!(
        (external_name(high), external_name(low)),
        (Some(high), Some(low)) if high == low
    )
}

fn external_name(relocation: &mwcc_machine_code::Relocation) -> Option<&str> {
    match &relocation.target {
        mwcc_machine_code::RelocationTarget::External(name) => Some(name),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_a_global_member_with_pass_through_arguments() {
        let target = Expression::Member {
            base: Box::new(Expression::Variable("table".into())),
            offset: 12,
            member_type: Type::Pointer(Pointee::Int),
            index_stride: None,
        };
        let arguments = [
            Expression::Variable("buffer".into()),
            Expression::Variable("length".into()),
        ];

        assert_eq!(
            global_member_forward_call(&target, &arguments),
            Some(("table", 12))
        );
        assert_eq!(
            global_member_forward_call(&target, &[]),
            Some(("table", 12))
        );
    }
}
