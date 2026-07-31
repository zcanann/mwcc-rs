//! MWCC schedule for a traced global queue append.

#[allow(unused_imports)]
use super::super::*;
use super::recognize::recognize;

fn pointer_word(value_type: Type) -> bool {
    matches!(value_type, Type::Pointer(_) | Type::StructPointer { .. })
}

impl Generator {
    pub(crate) fn try_global_doubly_linked_append_trace(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.global_addressing != GlobalAddressing::SmallData
            || !self.behavior.tail_call_optimization
            || self.behavior.string_literals_packed
        {
            return Ok(false);
        }
        let Some(shape) = recognize(function) else {
            return Ok(false);
        };
        if self.lookup_general(shape.item) != Some(Eabi::FIRST_GENERAL_ARGUMENT)
            || shape.string.len() + 1 <= 8
            || !self.variadic_callees.contains(shape.callee)
            || self.locations.contains_key(shape.callee)
            || self.globals.contains_key(shape.callee)
            || [shape.current, shape.head, shape.tail]
                .into_iter()
                .any(|name| !self.globals.get(name).copied().is_some_and(pointer_word))
        {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        let append_nonempty = self.fresh_label();
        let join = self.fresh_label();
        self.record_relocation(RelocationKind::EmbSda21, shape.tail);
        self.output.instructions.push(Instruction::LoadWord {
            d: 4,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 4, immediate: 0 });
        self.emit_branch_conditional_to(4, 2, append_nonempty);
        self.record_relocation(RelocationKind::EmbSda21, shape.current);
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.record_relocation(RelocationKind::EmbSda21, shape.tail);
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 0,
            offset: 0,
        });
        self.record_relocation(RelocationKind::EmbSda21, shape.head);
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 0,
            offset: 0,
        });
        self.output.instructions.extend([
            Instruction::StoreWord {
                s: 0,
                a: 3,
                offset: shape.previous,
            },
            Instruction::StoreWord {
                s: 0,
                a: 3,
                offset: shape.next,
            },
        ]);
        self.emit_branch_to(join);

        self.bind_label(append_nonempty);
        self.output.instructions.extend([
            Instruction::StoreWord {
                s: 3,
                a: 4,
                offset: shape.next,
            },
            Instruction::load_immediate(0, 0),
            Instruction::StoreWord {
                s: 0,
                a: 3,
                offset: shape.next,
            },
        ]);
        self.record_relocation(RelocationKind::EmbSda21, shape.tail);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 0,
            offset: 0,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: shape.previous,
        });
        self.record_relocation(RelocationKind::EmbSda21, shape.tail);
        self.output.instructions.push(Instruction::StoreWord {
            s: 3,
            a: 0,
            offset: 0,
        });

        self.bind_label(join);
        let string = self.string_literal_placeholder(shape.string);
        self.output
            .instructions
            .push(Instruction::load_immediate(0, 0));
        self.emit_address_high(5, &string);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 3,
            offset: shape.state,
        });
        self.output
            .instructions
            .push(Instruction::move_register(4, 3));
        self.emit_string_address_low(&string, 5, 3);
        self.output
            .instructions
            .push(Instruction::ConditionRegisterClear { d: 6 });
        self.record_relocation(RelocationKind::Rel24, shape.callee);
        self.output.instructions.push(Instruction::BranchExternal {
            target: shape.callee.to_string(),
        });
        Ok(true)
    }
}
