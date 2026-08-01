//! Shared entry-address lowering for file-scope pointer tables.

use super::*;

impl Generator {
    /// Load `global_table[index]` as a pointer value. A file-scope `T**`
    /// object first yields the table address; the subscript then uses pointer
    /// stride regardless of the aggregate size behind each selected entry.
    pub(crate) fn try_emit_global_pointer_table_entry(
        &mut self,
        name: &str,
        index: &Expression,
    ) -> Compilation<Option<u8>> {
        if !matches!(
            self.globals.get(name),
            Some(Type::Pointer(Pointee::Pointer | Pointee::WordPointer))
        ) || self.locations.contains_key(name)
        {
            return Ok(None);
        }

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
        Ok(Some(table))
    }
}
