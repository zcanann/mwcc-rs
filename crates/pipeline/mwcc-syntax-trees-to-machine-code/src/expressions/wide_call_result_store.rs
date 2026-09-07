//! Capture both words of an EABI integer call result before ordinary scalar
//! expression selection can discard its high word.

use super::*;

impl Generator {
    pub(crate) fn try_emit_wide_call_result_store(
        &mut self,
        target: &Expression,
        value: &Expression,
    ) -> Compilation<bool> {
        let Expression::Call { name, arguments } = value else {
            return Ok(false);
        };
        if !matches!(
            self.call_return_types.get(name),
            Some(Type::LongLong | Type::UnsignedLongLong)
        ) {
            return Ok(false);
        }
        let global = match target {
            Expression::Variable(global)
                if !self.known_locals.contains(global)
                    && !self.locations.contains_key(global)
                    && matches!(
                        self.globals.get(global),
                        Some(Type::LongLong | Type::UnsignedLongLong)
                    ) =>
            {
                Some(global)
            }
            _ => None,
        };
        let pointer = match target {
            Expression::Dereference { pointer }
                if !crate::analysis::expression_has_side_effect(pointer)
                    && matches!(
                        self.pointee_of(pointer),
                        Ok(Pointee::LongLong | Pointee::UnsignedLongLong)
                    ) =>
            {
                Some(pointer.as_ref())
            }
            _ => None,
        };
        if global.is_none() && pointer.is_none() {
            return Ok(false);
        }
        self.emit_call(name, arguments, None, false)?;
        if let Some(global) =
            global.filter(|_| self.behavior.global_addressing == GlobalAddressing::SmallData)
        {
            self.record_relocation_with_addend(RelocationKind::EmbSda21, global, 4);
            self.output.instructions.push(Instruction::StoreWord {
                s: 4,
                a: 0,
                offset: 0,
            });
            self.record_relocation(RelocationKind::EmbSda21, global);
            self.output.instructions.push(Instruction::StoreWord {
                s: 3,
                a: 0,
                offset: 0,
            });
        } else {
            let address = pointer.cloned().unwrap_or_else(|| Expression::AddressOf {
                operand: Box::new(target.clone()),
            });
            let added_high = self.reserved.insert(3);
            let added_low = self.reserved.insert(4);
            let base = self.fresh_virtual_general_preferring(5);
            let result = self.evaluate_general(&address, base);
            if added_high {
                self.reserved.remove(&3);
            }
            if added_low {
                self.reserved.remove(&4);
            }
            result?;
            self.output.instructions.push(Instruction::StoreWord {
                s: 4,
                a: base,
                offset: 4,
            });
            self.output.instructions.push(Instruction::StoreWord {
                s: 3,
                a: base,
                offset: 0,
            });
        }
        Ok(true)
    }
}
