//! Entry scheduling for dense structured frames.
//!
//! Once many incoming values must survive the first call, MWCC interleaves
//! their saved-home copies with an independent computed-address definition.
//! Keeping that schedule here lets the structured CFG owner remain concerned
//! with liveness and statement emission rather than one prologue permutation.

use super::guarded_computed_survivor::emit_scaled_index;
#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(super) fn emit_structured_dense_frame_entry(
        &mut self,
        function: &Function,
        saved_parameters: &[(String, u8, u8)],
    ) -> Compilation<bool> {
        let Some(Statement::Assign { name, value }) = function.statements.first() else {
            return Ok(false);
        };
        let Some(local) = function.locals.iter().find(|local| &local.name == name) else {
            return Ok(false);
        };
        let Type::StructPointer { element_size } = local.declared_type else {
            return Ok(false);
        };
        let Expression::AddressOf { operand } = value else {
            return Ok(false);
        };
        let Expression::Index { base, index } = operand.as_ref() else {
            return Ok(false);
        };
        let (Expression::Variable(global), Expression::Variable(index_name)) =
            (base.as_ref(), index.as_ref())
        else {
            return Ok(false);
        };
        if !self.global_array_sizes.contains_key(global) {
            return Ok(false);
        }
        let Some(&(.., index_home, index_incoming)) = saved_parameters
            .iter()
            .find(|(parameter, _, _)| parameter == index_name)
        else {
            return Ok(false);
        };
        if index_incoming != Eabi::FIRST_GENERAL_ARGUMENT {
            return Ok(false);
        }
        let Some(destination) = self.locations.get(name).map(|location| location.register) else {
            return Ok(false);
        };

        self.emit_callee_saved_home_copy(index_home, index_incoming);
        let (high_preference, scaled_preference) = match self.behavior.frame_convention {
            FrameConvention::LinkageFirst => (3, 4),
            FrameConvention::Predecrement => (8, 9),
        };
        let high = self.fresh_virtual_general_preferring(high_preference);
        let scaled = self.fresh_virtual_general_preferring(scaled_preference);
        self.emit_address_high(high, global);
        let remaining: Vec<(u8, u8)> = function
            .parameters
            .iter()
            .filter_map(|parameter| {
                saved_parameters
                    .iter()
                    .find(|(name, _, _)| name == &parameter.name && name != index_name)
                    .map(|(_, home, incoming)| (*home, *incoming))
            })
            .collect();
        if self.behavior.frame_convention == FrameConvention::Predecrement {
            emit_scaled_index(
                &mut self.output.instructions,
                scaled,
                index_incoming,
                element_size,
            )?;
        }
        if let Some(&(home, incoming)) = remaining.first() {
            self.emit_callee_saved_home_copy(home, incoming);
        }
        self.record_relocation(RelocationKind::Addr16Lo, global);
        self.output.instructions.push(Instruction::AddImmediate {
            d: GENERAL_SCRATCH,
            a: high,
            immediate: 0,
        });
        if self.behavior.frame_convention == FrameConvention::LinkageFirst {
            emit_scaled_index(
                &mut self.output.instructions,
                scaled,
                index_home,
                element_size,
            )?;
        }
        for (remaining_index, &(home, incoming)) in remaining.iter().enumerate().skip(1) {
            self.emit_callee_saved_home_copy(home, incoming);
            if self.behavior.frame_convention == FrameConvention::LinkageFirst
                && remaining_index == 1
            {
                self.output.instructions.push(Instruction::Add {
                    d: destination,
                    a: GENERAL_SCRATCH,
                    b: scaled,
                });
            }
        }
        if self.behavior.frame_convention == FrameConvention::Predecrement {
            self.output.instructions.push(Instruction::Add {
                d: destination,
                a: GENERAL_SCRATCH,
                b: scaled,
            });
        }
        Ok(true)
    }
}
