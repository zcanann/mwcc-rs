//! A nested call whose result is stored to and reloaded from a global array.
//!
//! The index and two later call arguments cross the inner call. Legacy mwcc
//! parks them in r29-r31, computes the scaled index once after the outer call,
//! and reuses both that index and the array base for the store/reload pair.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Lower
    /// `g[index] = outer(inner(), narrow, 0, 0, pointer); return g[index];`.
    pub(crate) fn try_indexed_call_store_return(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if self.behavior.frame_convention != FrameConvention::Predecrement
            || self.behavior.scheduler_enabled
            || self.behavior.use_lmw_stmw
            || !function.locals.is_empty()
            || !function.guards.is_empty()
            || function.parameters.len() != 3
            || !matches!(function.return_type, Type::Int | Type::UnsignedInt)
        {
            return Ok(false);
        }
        let Some(return_expression) = function.return_expression.as_ref() else {
            return Ok(false);
        };

        let [Statement::Store {
            target,
            value:
                Expression::Call {
                    name: outer,
                    arguments,
                },
        }] = function.statements.as_slice()
        else {
            return Ok(false);
        };
        let Expression::Index { base, index } = target else {
            return Ok(false);
        };
        let Expression::Variable(global) = base.as_ref() else {
            return Ok(false);
        };
        let Expression::Variable(index_name) = index.as_ref() else {
            return Ok(false);
        };
        if !structurally_equal(target, return_expression)
            || !self
                .global_array_sizes
                .get(global.as_str())
                .is_some_and(|size| *size > 8 && *size % 4 == 0)
            || self.globals.get(global.as_str()).copied() != Some(function.return_type)
        {
            return Ok(false);
        }

        let [first, Expression::Variable(narrow_name), third, fourth, Expression::Variable(pointer_name)] =
            arguments.as_slice()
        else {
            return Ok(false);
        };
        let Expression::Call {
            name: inner,
            arguments: inner_arguments,
        } = first
        else {
            return Ok(false);
        };
        let [index_parameter, narrow_parameter, pointer_parameter] =
            function.parameters.as_slice()
        else {
            return Ok(false);
        };
        if !inner_arguments.is_empty()
            || index_parameter.name != *index_name
            || narrow_parameter.name != *narrow_name
            || pointer_parameter.name != *pointer_name
            || !matches!(index_parameter.parameter_type, Type::Int | Type::UnsignedInt)
            || !matches!(narrow_parameter.parameter_type, Type::Short | Type::UnsignedShort)
            || !matches!(pointer_parameter.parameter_type, Type::Pointer(_) | Type::StructPointer { .. })
            || !is_zero_literal(third)
            || !is_zero_literal(fourth)
        {
            return Ok(false);
        }

        self.non_leaf = true;
        self.frame_size = 32;
        self.callee_saved = vec![29, 30, 31];
        self.output.pre_scheduled = true;

        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -32,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 36,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 11,
            a: 1,
            immediate: 32,
        });
        self.record_relocation(RelocationKind::Rel24, "_savegpr_29");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "_savegpr_29".to_string(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(29, 3));
        self.output
            .instructions
            .push(Instruction::move_register(30, 4));
        self.output
            .instructions
            .push(Instruction::move_register(31, 5));

        self.record_relocation(RelocationKind::Rel24, inner);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: inner.clone(),
        });
        self.output
            .instructions
            .push(Instruction::move_register(4, 30));
        self.output
            .instructions
            .push(Instruction::load_immediate(5, 0));
        self.output
            .instructions
            .push(Instruction::load_immediate(6, 0));
        self.output
            .instructions
            .push(Instruction::move_register(7, 31));
        self.record_relocation(RelocationKind::Rel24, outer);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: outer.clone(),
        });

        self.output
            .instructions
            .push(Instruction::ShiftLeftImmediate {
                a: 0,
                s: 29,
                shift: 2,
            });
        self.emit_address_high(4, global);
        self.emit_address_low(4, global);
        self.output
            .instructions
            .push(Instruction::StoreWordIndexed { s: 3, a: 4, b: 0 });
        self.output
            .instructions
            .push(Instruction::LoadWordIndexed { d: 3, a: 4, b: 0 });

        self.output.instructions.push(Instruction::AddImmediate {
            d: 11,
            a: 1,
            immediate: 32,
        });
        self.record_relocation(RelocationKind::Rel24, "_restgpr_29");
        self.output.instructions.push(Instruction::BranchAndLink {
            target: "_restgpr_29".to_string(),
        });
        self.output.instructions.push(Instruction::LoadWord {
            d: 0,
            a: 1,
            offset: 36,
        });
        self.output
            .instructions
            .push(Instruction::MoveToLinkRegister { s: 0 });
        self.output.instructions.push(Instruction::AddImmediate {
            d: 1,
            a: 1,
            immediate: 32,
        });
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
