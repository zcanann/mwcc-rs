//! Straight-line initialization of one global aggregate between calls that use
//! its address. The aggregate base is a single value live across every call.

#[allow(unused_imports)]
use super::*;

fn call_with_global_address(statement: &Statement) -> Option<(&str, &str)> {
    let Statement::Expression(Expression::Call { name, arguments }) = statement else {
        return None;
    };
    let [Expression::AddressOf { operand }] = arguments.as_slice() else {
        return None;
    };
    let Expression::Variable(global) = operand.as_ref() else {
        return None;
    };
    Some((name, global))
}

fn constant_member_store(statement: &Statement) -> Option<(&str, u16, i16)> {
    let Statement::Store {
        target:
            Expression::Member {
                base,
                offset,
                member_type,
                index_stride: None,
            },
        value,
    } = statement
    else {
        return None;
    };
    if !matches!(member_type, Type::Int | Type::UnsignedInt) {
        return None;
    }
    let Expression::Variable(global) = base.as_ref() else {
        return None;
    };
    Some((
        global,
        u16::try_from(*offset).ok()?,
        i16::try_from(constant_value(value)?).ok()?,
    ))
}

impl Generator {
    /// Emit the legacy SDK queue-initialization schedule:
    ///
    /// `open(&g); lock(&g); g.a=Z; g.b=Z; g.c=K; close(&g); return Z;`
    ///
    /// Build 163 holds the one shared aggregate base in r31. The first two
    /// equal stores reuse r3; the distinct tail value occupies r0, allowing the
    /// final call argument to be prepared before that tail store. Keeping this
    /// family here prevents the general statement emitter from rematerializing
    /// `g` separately for every call/store.
    pub(crate) fn try_global_aggregate_call_initialization(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::LinkageFirst
            || !self.frame_slots.is_empty()
            || !function.parameters.is_empty()
            || !function.locals.is_empty()
            || !function.guards.is_empty()
            || !matches!(function.return_type, Type::Int | Type::UnsignedInt)
        {
            return Ok(false);
        }
        let Some(return_value) = function
            .return_expression
            .as_ref()
            .and_then(constant_value)
            .and_then(|value| i16::try_from(value).ok())
        else {
            return Ok(false);
        };
        let [first_call, second_call, first_store, second_store, third_store, final_call] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        let (first_callee, global) = match call_with_global_address(first_call) {
            Some(call) => call,
            None => return Ok(false),
        };
        let Some((second_callee, second_global)) = call_with_global_address(second_call) else {
            return Ok(false);
        };
        let Some((final_callee, final_global)) = call_with_global_address(final_call) else {
            return Ok(false);
        };
        let Some((store0_global, offset0, value0)) = constant_member_store(first_store) else {
            return Ok(false);
        };
        let Some((store1_global, offset1, value1)) = constant_member_store(second_store) else {
            return Ok(false);
        };
        let Some((store2_global, offset2, value2)) = constant_member_store(third_store) else {
            return Ok(false);
        };
        if second_global != global
            || final_global != global
            || store0_global != global
            || store1_global != global
            || store2_global != global
            || value0 != value1
            || !(offset0 < offset1 && offset1 < offset2)
            || !matches!(self.globals.get(global), Some(Type::Struct { size, .. }) if *size > 8)
        {
            return Ok(false);
        }

        self.non_leaf = true;
        self.callee_saved = vec![31];
        self.frame_size = 16;
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.record_relocation(RelocationKind::Addr16Ha, global);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(3, 0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            });
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 12,
        });
        self.record_relocation(RelocationKind::Addr16Lo, global);
        self.output.instructions.push(Instruction::AddImmediate {
            d: 31,
            a: 3,
            immediate: 0,
        });

        // The initial collision-resolving copy uses addi in build 163; later
        // forwarding copies use mr.
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 31,
            immediate: 0,
        });
        self.record_relocation(RelocationKind::Rel24, first_callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: first_callee.to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.record_relocation(RelocationKind::Rel24, second_callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: second_callee.to_string(),
        });

        self.output
            .instructions
            .push(Instruction::load_immediate(3, value0));
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 31,
            offset: offset0 as i16,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, value2));
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 31,
            offset: offset1 as i16,
        });
        self.output
            .instructions
            .push(Instruction::move_register(3, 31));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 31,
            offset: offset2 as i16,
        });
        self.record_relocation(RelocationKind::Rel24, final_callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: final_callee.to_string(),
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(3, return_value));
        // This queue family restores the saved base before exposing the caller
        // linkage area again, then reloads LR through the restored stack pointer.
        self.output.instructions.push(Instruction::LoadWord {
            d: 31,
            a: 1,
            offset: 12,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 16,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 4,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
