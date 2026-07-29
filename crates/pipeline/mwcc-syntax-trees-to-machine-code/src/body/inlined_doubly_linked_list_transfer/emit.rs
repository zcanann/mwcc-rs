use super::*;
use recognize::ListTransfer;

impl Generator {
    pub(super) fn emit_legacy_list_transfer(&mut self, shape: &ListTransfer<'_>) {
        const DESCRIPTOR: u8 = 31;
        const CELL: u8 = 6;

        self.non_leaf = true;
        self.frame_size = 24;
        self.callee_saved = vec![DESCRIPTOR];
        self.output.pre_scheduled = true;
        let after_next_repair = self.fresh_label();
        let repair_previous = self.fresh_label();
        let extracted = self.fresh_label();

        self.output.instructions.extend([
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::AddImmediate {
                d: CELL,
                a: 4,
                immediate: -shape.cell_header_size,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            },
            Instruction::MultiplyImmediate {
                d: 0,
                a: 3,
                immediate: shape.descriptor_stride,
            },
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -24,
            },
            Instruction::StoreWord {
                s: DESCRIPTOR,
                a: 1,
                offset: 20,
            },
        ]);
        self.record_relocation(RelocationKind::EmbSda21, shape.heap_array);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 4,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 3,
                a: CELL,
                offset: shape.next_offset,
            },
            Instruction::Add {
                d: DESCRIPTOR,
                a: 4,
                b: 0,
            },
            Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 },
            Instruction::LoadWord {
                d: 5,
                a: DESCRIPTOR,
                offset: shape.allocated_offset,
            },
            Instruction::move_register(4, CELL),
        ]);
        self.emit_branch_conditional_to(12, 2, after_next_repair);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 4,
                offset: shape.previous_offset,
            },
            Instruction::StoreWord {
                s: 0,
                a: 3,
                offset: shape.previous_offset,
            },
        ]);
        self.bind_label(after_next_repair);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 3,
                a: 4,
                offset: shape.previous_offset,
            },
            Instruction::CompareLogicalWordImmediate { a: 3, immediate: 0 },
        ]);
        self.emit_branch_conditional_to(4, 2, repair_previous);
        self.output.instructions.push(Instruction::LoadWord {
            d: 5,
            a: 4,
            offset: shape.next_offset,
        });
        self.emit_branch_to(extracted);
        self.bind_label(repair_previous);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 4,
                offset: shape.next_offset,
            },
            Instruction::StoreWord {
                s: 0,
                a: 3,
                offset: shape.next_offset,
            },
        ]);
        self.bind_label(extracted);
        self.output.instructions.extend([
            Instruction::StoreWord {
                s: 5,
                a: DESCRIPTOR,
                offset: shape.allocated_offset,
            },
            Instruction::LoadWord {
                d: 3,
                a: DESCRIPTOR,
                offset: shape.free_offset,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, shape.insert_helper);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.insert_helper.to_string(),
        });
        self.output.instructions.extend([
            Instruction::StoreWord {
                s: 3,
                a: DESCRIPTOR,
                offset: shape.free_offset,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 28,
            },
            Instruction::LoadWord {
                d: DESCRIPTOR,
                a: 1,
                offset: 20,
            },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 24,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::BranchToLinkRegister,
        ]);
    }

    pub(super) fn emit_modern_list_transfer(&mut self, shape: &ListTransfer<'_>) {
        const DESCRIPTOR: u8 = 31;

        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved = vec![DESCRIPTOR];
        self.output.pre_scheduled = true;
        let after_next_repair = self.fresh_label();
        let repair_previous = self.fresh_label();
        let extracted = self.fresh_label();

        self.output.instructions.extend([
            Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            },
            Instruction::MoveFromLinkRegister { d: 0 },
            Instruction::AddImmediate {
                d: 4,
                a: 4,
                immediate: -shape.cell_header_size,
            },
            Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 20,
            },
            Instruction::MultiplyImmediate {
                d: 0,
                a: 3,
                immediate: shape.descriptor_stride,
            },
            Instruction::StoreWord {
                s: DESCRIPTOR,
                a: 1,
                offset: 12,
            },
        ]);
        self.record_relocation(RelocationKind::EmbSda21, shape.heap_array);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 5,
                a: 0,
                offset: 0,
            },
            Instruction::LoadWord {
                d: 6,
                a: 4,
                offset: shape.next_offset,
            },
            Instruction::Add {
                d: DESCRIPTOR,
                a: 5,
                b: 0,
            },
            Instruction::CompareLogicalWordImmediate { a: 6, immediate: 0 },
            Instruction::LoadWord {
                d: 3,
                a: DESCRIPTOR,
                offset: shape.allocated_offset,
            },
        ]);
        self.emit_branch_conditional_to(12, 2, after_next_repair);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 4,
                offset: shape.previous_offset,
            },
            Instruction::StoreWord {
                s: 0,
                a: 6,
                offset: shape.previous_offset,
            },
        ]);
        self.bind_label(after_next_repair);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 5,
                a: 4,
                offset: shape.previous_offset,
            },
            Instruction::CompareLogicalWordImmediate { a: 5, immediate: 0 },
        ]);
        self.emit_branch_conditional_to(4, 2, repair_previous);
        self.output.instructions.push(Instruction::LoadWord {
            d: 3,
            a: 4,
            offset: shape.next_offset,
        });
        self.emit_branch_to(extracted);
        self.bind_label(repair_previous);
        self.output.instructions.extend([
            Instruction::LoadWord {
                d: 0,
                a: 4,
                offset: shape.next_offset,
            },
            Instruction::StoreWord {
                s: 0,
                a: 5,
                offset: shape.next_offset,
            },
        ]);
        self.bind_label(extracted);
        self.output.instructions.extend([
            Instruction::StoreWord {
                s: 3,
                a: DESCRIPTOR,
                offset: shape.allocated_offset,
            },
            Instruction::LoadWord {
                d: 3,
                a: DESCRIPTOR,
                offset: shape.free_offset,
            },
        ]);
        self.record_relocation(RelocationKind::Rel24, shape.insert_helper);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: shape.insert_helper.to_string(),
        });
        self.output.instructions.extend([
            Instruction::StoreWord {
                s: 3,
                a: DESCRIPTOR,
                offset: shape.free_offset,
            },
            Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: 20,
            },
            Instruction::LoadWord {
                d: DESCRIPTOR,
                a: 1,
                offset: 12,
            },
            Instruction::MoveToLinkRegister { s: 0 },
            Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: 16,
            },
            Instruction::BranchToLinkRegister,
        ]);
    }
}
