//! Pointer values selected from a pointer table.
//!
//! A nested subscript such as `object->rows[row][column]` first loads the
//! pointer-table member, selects one pointer from it, then uses that pointer as
//! the address for the outer subscript. Keeping that address transaction here
//! lets ordinary subscript lowering remain concerned only with the final
//! scalar element.

#[allow(unused_imports)]
use super::*;

impl Generator {
    fn materialize_word_pointer_table(&mut self, base: &Expression) -> Compilation<Option<u8>> {
        if self.pointee_of(base).ok() != Some(Pointee::WordPointer) {
            return Ok(None);
        }
        // A member-backed table gets a distinct high volatile lane. MWCC keeps
        // the owner (`this`) and its indices intact while the selected pointer
        // occupies the result lane.
        let table = if matches!(base, Expression::Member { .. }) {
            let table = self.fresh_virtual_general_preferring(6);
            self.evaluate_general(base, table)?;
            table
        } else {
            self.resolve_pointer(base)?.1
        };
        Ok(Some(table))
    }

    /// Emit the complete `table[row][column]` word-load transaction. MWCC
    /// prepares both scaled indices before chasing the selected row pointer;
    /// treating the inner index as an ordinary completed expression reverses
    /// those two instructions and loses byte parity.
    pub(crate) fn try_emit_nested_pointer_table_subscript(
        &mut self,
        base: &Expression,
        index: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        let Expression::Index {
            base: table_base,
            index: table_index,
        } = base
        else {
            return Ok(false);
        };
        if destination == GENERAL_SCRATCH {
            return Ok(false);
        }
        let Some(table) = self.materialize_word_pointer_table(table_base)? else {
            return Ok(false);
        };

        let row = self.materialize_index_operand(table_index)?;
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: destination,
                s: row,
                shift: 2,
            });
        let column = self.materialize_index_operand(index)?;
        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: GENERAL_SCRATCH,
                s: column,
                shift: 2,
            });
        self.output
            .instructions
            .push(Instruction::LoadWordIndexed {
                d: destination,
                a: table,
                b: destination,
            });
        self.output
            .instructions
            .push(Instruction::LoadWordIndexed {
                d: destination,
                a: destination,
                b: GENERAL_SCRATCH,
            });
        Ok(true)
    }

    /// Materialize the pointer yielded by indexing a word-pointer table.
    ///
    /// `WordPointer` is the IR's proof that the selected pointer's eventual
    /// scalar load is a word. Opaque pointer tables deliberately stay deferred:
    /// their final element could require a byte, halfword, or floating load.
    pub(crate) fn try_resolve_nested_pointer_table_entry(
        &mut self,
        expression: &Expression,
    ) -> Compilation<Option<(Pointee, u8)>> {
        let Expression::Index { base, index } = expression else {
            return Ok(None);
        };
        let Some(table) = self.materialize_word_pointer_table(base)? else {
            return Ok(None);
        };
        let index_register = self.materialize_index_operand(index)?;
        let selected = self.fresh_virtual_general_preferring(3);
        let scaled = if crate::analysis::is_prescaled_pointer_table_index(index) {
            index_register
        } else {
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: selected,
                    s: index_register,
                    shift: 2,
                });
            selected
        };
        self.output
            .instructions
            .push(Instruction::LoadWordIndexed {
                d: selected,
                a: table,
                b: scaled,
            });
        Ok(Some((Pointee::UnsignedInt, selected)))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use mwcc_syntax_trees::{
        Function, InlineExpansionFacts, Parameter, SourceFundamentalType,
    };
    use mwcc_versions::{CompilerConfig, GC_2_0};

    use super::*;
    use crate::{lower_function, InlineBodySet, InlineSummaries};

    #[test]
    fn lowers_a_word_pointer_selected_from_a_member_table() {
        let table = Expression::Member {
            base: Box::new(Expression::Variable("this".into())),
            offset: 24,
            member_type: Type::Pointer(Pointee::WordPointer),
            index_stride: None,
        };
        let row = Expression::Index {
            base: Box::new(table),
            index: Box::new(Expression::Variable("row".into())),
        };
        let function = Function {
            return_type: Type::Int,
            name: "GetChild__11cSHierarchyCFii".into(),
            is_static: false,
            is_weak: false,
            parameters: vec![
                Parameter {
                    parameter_type: Type::StructPointer { element_size: 52 },
                    name: "this".into(),
                },
                Parameter {
                    parameter_type: Type::Int,
                    name: "row".into(),
                },
                Parameter {
                    parameter_type: Type::Int,
                    name: "column".into(),
                },
            ],
            locals: Vec::new(),
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: Some(Expression::Index {
                base: Box::new(row),
                index: Box::new(Expression::Variable("column".into())),
            }),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: true,
        };
        let fundamentals = HashMap::from([(
            function.name.clone(),
            SourceFundamentalType::SignedInteger,
        )]);
        let mut config = CompilerConfig::new(GC_2_0);
        config.flags.cpp_exceptions = false;

        let machine = lower_function(
            &function,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashMap::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            &HashMap::new(),
            &HashMap::new(),
            &InlineBodySet::default(),
            &InlineSummaries::default(),
            InlineExpansionFacts::default(),
            &HashMap::new(),
            &fundamentals,
            config,
        )
        .expect("member pointer-table subscript should lower");

        assert_eq!(
            machine.encode_text(),
            [
                0x80c3_0018_u32,
                0x5483_103a,
                0x54a0_103a,
                0x7c66_182e,
                0x7c63_002e,
                0x4e80_0020,
            ]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect::<Vec<_>>()
        );
    }
}
