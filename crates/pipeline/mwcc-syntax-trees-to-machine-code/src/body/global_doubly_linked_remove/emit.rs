//! MWCC schedule for global intrusive-queue removal.

#[allow(unused_imports)]
use super::super::*;
use super::recognize::recognize;

fn pointer_word(value_type: Type) -> bool {
    matches!(value_type, Type::Pointer(_) | Type::StructPointer { .. })
}

impl Generator {
    pub(crate) fn try_global_doubly_linked_remove(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.global_addressing != GlobalAddressing::SmallData {
            return Ok(false);
        }
        let Some(shape) = recognize(function) else {
            return Ok(false);
        };
        if self.lookup_general(shape.item) != Some(Eabi::FIRST_GENERAL_ARGUMENT)
            || [shape.current, shape.head, shape.tail]
                .into_iter()
                .any(|name| !self.globals.get(name).copied().is_some_and(pointer_word))
        {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        let remove_tail = self.fresh_label();
        let empty = self.fresh_label();
        let remove_middle = self.fresh_label();

        self.output.instructions.extend([
            Instruction::load_immediate(4, 0),
            Instruction::load_immediate(0, 3),
            Instruction::StoreWord {
                s: 4,
                a: 3,
                offset: shape.flags,
            },
            Instruction::StoreWord {
                s: 0,
                a: 3,
                offset: shape.state,
            },
        ]);
        self.record_relocation(RelocationKind::EmbSda21, shape.head);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 0, b: 3 });
        self.emit_branch_conditional_to(4, 2, remove_tail);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: shape.next,
        });
        self.output
            .instructions
            .push(Instruction::CompareWordImmediate { a: 0, immediate: 0 });
        self.emit_branch_conditional_to(12, 2, empty);
        self.record_relocation(RelocationKind::EmbSda21, shape.head);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 3,
            offset: shape.next,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 3,
            offset: shape.previous,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);

        self.bind_label(empty);
        for global in [shape.current, shape.tail, shape.head] {
            self.record_relocation(RelocationKind::EmbSda21, global);
            self.output.instructions.push(Instruction::StoreWord {
                s: 4,
                a: 0,
                offset: 0,
            });
        }
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);

        self.bind_label(remove_tail);
        self.record_relocation(RelocationKind::EmbSda21, shape.tail);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord { a: 0, b: 3 });
        self.emit_branch_conditional_to(4, 2, remove_middle);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: shape.previous,
        });
        self.record_relocation(RelocationKind::EmbSda21, shape.tail);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 3,
            offset: shape.previous,
        });
        self.output.instructions.push(Instruction::StoreWord {
            s: 4,
            a: 3,
            offset: shape.next,
        });
        self.record_relocation(RelocationKind::EmbSda21, shape.head);
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 0,
            offset: 0,
        });
        self.record_relocation(RelocationKind::EmbSda21, shape.current);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);

        self.bind_label(remove_middle);
        self.output.instructions.extend([Instruction::LoadWord {
            d: 0,
            a: 3,
            offset: shape.next,
        }]);
        self.record_relocation(RelocationKind::EmbSda21, shape.current);
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 0,
            offset: 0,
        });
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: shape.next,
            },
            Instruction::LoadWord {
                d: 4,
                a: 3,
                offset: shape.previous,
            },
            Instruction::StoreWord {
                s: 0,
                a: 4,
                offset: shape.next,
            },
            Instruction::LoadWord {
                d: 0,
                a: 3,
                offset: shape.previous,
            },
            Instruction::LoadWord {
                d: 3,
                a: 3,
                offset: shape.next,
            },
            Instruction::StoreWord {
                s: 0,
                a: 3,
                offset: shape.previous,
            },
            Instruction::BranchToLinkRegister,
        ]);
        Ok(true)
    }
}
