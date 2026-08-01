//! Member stores through entries loaded from a file-scope pointer table.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Emit `global_table[index]->member = value` where `global_table` is a
    /// scalar `T**` object. The global first yields the table address; indexing
    /// that table has a four-byte pointer stride, and the selected entry then
    /// supplies the member-store base. The parser's retained aggregate stride
    /// describes `T`, not the pointer table, so generic struct-array lowering
    /// cannot own this indirection.
    pub(crate) fn try_emit_global_pointer_table_member_store(
        &mut self,
        name: &str,
        index: &Expression,
        member_offset: u32,
        pointee: Pointee,
        value: &Expression,
    ) -> Compilation<bool> {
        if !matches!(
            self.globals.get(name),
            Some(Type::Pointer(Pointee::Pointer | Pointee::WordPointer))
        ) || self.locations.contains_key(name)
        {
            return Ok(false);
        }
        let member_offset = i16::try_from(member_offset).map_err(|_| {
            Diagnostic::error("global pointer-table member displacement is out of range")
        })?;

        let table = self.fresh_virtual_general_preferring(4);
        self.emit_global_load(name, table)?;
        let index = self.materialize_index_operand(index)?;
        let scaled = self.fresh_virtual_general_preferring(5);
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: scaled,
                s: index,
                shift: 2,
            });
        self.output.instructions.push(Instruction::LoadWordIndexed {
            d: table,
            a: table,
            b: scaled,
        });

        let restore = self.reserved.insert(table);
        let source = self.place_store_value(value, pointee)?;
        if restore {
            self.reserved.remove(&table);
        }
        self.output.instructions.push(displacement_store(
            pointee,
            source,
            table,
            member_offset,
        )?);
        Ok(true)
    }
}
