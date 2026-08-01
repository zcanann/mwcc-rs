//! Function-address arguments to a callback stored in a global record.
//!
//! The first ABI lane is free until the ordinary argument is marshaled.  MWCC
//! uses it as the global-record address, begins the callback-address pair in
//! r4, loads the indirect callee, and only then overwrites r3 with the argument.

use super::*;

impl Generator {
    pub(super) fn try_emit_global_member_callback_indirect_call(
        &mut self,
        target: &Expression,
        arguments: &[Expression],
    ) -> Compilation<bool> {
        let Some((global, offset, argument, callback)) =
            global_member_callback_call(target, arguments)
        else {
            return Ok(false);
        };
        if (!self.addressable_globals.contains_key(global) && !self.globals.contains_key(global))
            || !self.is_direct_function_symbol(callback)
        {
            return Ok(false);
        }
        let (source, width, _) = self.leaf_info(argument)?;
        if width != 32 || source == 12 || source == Eabi::FIRST_GENERAL_ARGUMENT + 1 {
            return Ok(false);
        }
        let offset = i16::try_from(offset)
            .map_err(|_| Diagnostic::error("global callback member offset is out of range"))?;

        let base = Eabi::FIRST_GENERAL_ARGUMENT;
        let callback_lane = Eabi::FIRST_GENERAL_ARGUMENT + 1;
        self.emit_address_high(base, global);
        self.record_relocation(RelocationKind::Addr16Lo, global);
        self.output.instructions.push(Instruction::AddImmediate {
            d: base,
            a: base,
            immediate: 0,
        });
        self.emit_address_high(callback_lane, callback);
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: base,
            offset,
        });
        self.record_relocation(RelocationKind::Addr16Lo, callback);
        self.output.instructions.push(Instruction::AddImmediate {
            d: callback_lane,
            a: callback_lane,
            immediate: 0,
        });
        if source != base {
            self.evaluate_general(argument, base)?;
        }
        self.emit_indirect_branch_and_link(12);
        Ok(true)
    }

    pub(crate) fn schedule_linkage_first_global_member_callback(&mut self) {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !is_interleaved_linkage_entry(&self.output)
        {
            return;
        }
        crate::move_instruction_before_retargeting(self, 2, 1);
        crate::move_instruction_before_retargeting(self, 5, 2);
    }
}

fn is_interleaved_linkage_entry(output: &mwcc_machine_code::MachineFunction) -> bool {
    if !matches!(output.instructions.get(..9), Some([
        Instruction::MoveFromLinkRegister { d: 0 },
        Instruction::AddImmediateShifted { d: 3, a: 0, .. },
        Instruction::StoreWord { s: 0, a: 1, offset: 4 },
        Instruction::AddImmediate { d: 3, a: 3, immediate: 0 },
        Instruction::AddImmediateShifted { d: 4, a: 0, .. },
        Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -8 },
        Instruction::LoadWord { d: 12, a: 3, .. },
        Instruction::AddImmediate { d: 4, a: 4, immediate: 0 },
        Instruction::Or { a: 3, s: 6, b: 6 },
    ])) {
        return false;
    }
    let relocation = |index, kind| {
        output.relocations.iter().find(|relocation| {
            relocation.instruction_index == index && relocation.kind == kind
        })
    };
    let (Some(global_high), Some(global_low), Some(callback_high), Some(callback_low)) = (
        relocation(1, RelocationKind::Addr16Ha),
        relocation(3, RelocationKind::Addr16Lo),
        relocation(4, RelocationKind::Addr16Ha),
        relocation(7, RelocationKind::Addr16Lo),
    ) else {
        return false;
    };
    matches!(
        (
            external_name(global_high),
            external_name(global_low),
            external_name(callback_high),
            external_name(callback_low),
        ),
        (Some(global_high), Some(global_low), Some(callback_high), Some(callback_low))
            if global_high == global_low
                && callback_high == callback_low
                && global_high != callback_high
    )
}

fn external_name(relocation: &mwcc_machine_code::Relocation) -> Option<&str> {
    match &relocation.target {
        mwcc_machine_code::RelocationTarget::External(name) => Some(name),
        _ => None,
    }
}

fn global_member_callback_call<'a>(
    target: &'a Expression,
    arguments: &'a [Expression],
) -> Option<(&'a str, u32, &'a Expression, &'a str)> {
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
    let [argument, Expression::Variable(callback)] = arguments else {
        return None;
    };
    Some((global, *offset, argument, callback))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::{MachineFunction, Relocation, RelocationTarget};

    #[test]
    fn recognizes_a_global_member_call_with_a_callback_argument() {
        let target = Expression::Member {
            base: Box::new(Expression::Variable("table".into())),
            offset: 12,
            member_type: Type::Pointer(Pointee::Int),
            index_stride: None,
        };
        let arguments = [
            Expression::Variable("context".into()),
            Expression::Variable("callback".into()),
        ];

        assert!(matches!(
            global_member_callback_call(&target, &arguments),
            Some(("table", 12, Expression::Variable(context), "callback"))
                if context == "context"
        ));
    }

    #[test]
    fn recognizes_the_interleaved_linkage_entry() {
        let mut output = MachineFunction::default();
        output.instructions = vec![
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::AddImmediateShifted { d: 3, a: 0, immediate: 0 },
            Instruction::StoreWord { s: 0, a: 1, offset: 4 },
            Instruction::AddImmediate { d: 3, a: 3, immediate: 0 },
            Instruction::AddImmediateShifted { d: 4, a: 0, immediate: 0 },
            Instruction::StoreWordWithUpdate { s: 1, a: 1, offset: -8 },
            Instruction::LoadWord { d: 12, a: 3, offset: 0 },
            Instruction::AddImmediate { d: 4, a: 4, immediate: 0 },
            Instruction::Or { a: 3, s: 6, b: 6 },
        ];
        for (instruction_index, kind, target) in [
            (1, RelocationKind::Addr16Ha, "table"),
            (3, RelocationKind::Addr16Lo, "table"),
            (4, RelocationKind::Addr16Ha, "callback"),
            (7, RelocationKind::Addr16Lo, "callback"),
        ] {
            output.relocations.push(Relocation {
                instruction_index,
                kind,
                target: RelocationTarget::External(target.into()),
            });
        }

        assert!(is_interleaved_linkage_entry(&output));
    }
}
