//! Value-producing postfix increment/decrement.
//!
//! A postfix step has two results with overlapping lifetimes: the expression
//! yields the old value while the lvalue receives the stepped value. Keeping
//! that split here prevents the ordinary assignment path from accidentally
//! returning the new value.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn emit_post_step_value(
        &mut self,
        target: &Expression,
        operator: BinaryOperator,
        pointer_link: Option<(u32, u32)>,
        destination: u8,
    ) -> Compilation<()> {
        if pointer_link.is_some() {
            return Err(Diagnostic::error(
                "an overloaded iterator postfix step used as a value needs iterator lowering",
            ));
        }
        let Expression::Variable(name) = target else {
            return Err(Diagnostic::error(
                "a postfix step on this lvalue is not supported yet (roadmap)",
            ));
        };
        let Some(&value_type) = self.globals.get(name.as_str()) else {
            return Err(Diagnostic::error(
                "a local postfix step used as a value is not supported yet (roadmap)",
            ));
        };
        if !matches!(
            value_type,
            Type::Int | Type::UnsignedInt | Type::Pointer(_) | Type::StructPointer { .. }
        ) {
            return Err(Diagnostic::error(
                "a postfix step value currently requires a word-sized integer or pointer global",
            ));
        }
        let amount = match value_type {
            Type::Pointer(pointee) => i16::from(pointee.size()),
            Type::StructPointer { element_size } => i16::try_from(element_size)
                .map_err(|_| Diagnostic::error("postfix pointer stride is out of range"))?,
            _ => 1,
        };
        let amount = match operator {
            BinaryOperator::Add => amount,
            BinaryOperator::Subtract => amount
                .checked_neg()
                .ok_or_else(|| Diagnostic::error("postfix pointer stride is out of range"))?,
            _ => {
                return Err(Diagnostic::error(
                    "a postfix step requires increment or decrement",
                ))
            }
        };
        let old_value = if destination >= 14 || mwcc_vreg::Reg::is_virtual_field(destination) {
            let mut avoid = Vec::with_capacity(self.reserved.len() + 1);
            avoid.push(GENERAL_SCRATCH);
            avoid.extend(self.reserved.iter().copied());
            avoid.sort_unstable();
            avoid.dedup();
            self.fresh_virtual_general_avoiding(avoid)
        } else {
            destination
        };
        self.emit_global_load(name, old_value)?;
        self.output.instructions.push(Instruction::AddImmediate {
            d: GENERAL_SCRATCH,
            a: old_value,
            immediate: amount,
        });
        let copy_after_store = self.behavior.materialization_copy_style
            == mwcc_versions::MaterializationCopyStyle::AddImmediateZero;
        if old_value != destination && !copy_after_store {
            self.output
                .instructions
                .push(Instruction::move_register(destination, old_value));
        }
        self.emit_global_store(
            name,
            pointee_of_type(value_type).expect("word post-step type is scalar"),
            GENERAL_SCRATCH,
        )?;
        if old_value != destination && copy_after_store {
            self.output
                .instructions
                .push(Instruction::move_register(destination, old_value));
        }
        Ok(())
    }
}
