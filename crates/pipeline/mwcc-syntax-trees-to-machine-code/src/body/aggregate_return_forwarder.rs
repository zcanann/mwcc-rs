//! Hidden-result forwarding for aggregate-returning wrapper functions.
//!
//! An EABI aggregate result occupies an implicit first argument in r3.  A
//! wrapper returning another aggregate call must preserve that address while
//! shifting its source arguments and forwarding the same address to the
//! callee.  Keep this parallel-copy schedule separate from ordinary scalar
//! call lowering: the hidden value has no source-level `Parameter` or
//! `Expression`.

use super::*;
use mwcc_versions::FrameConvention;

struct AggregateReturnForwarder<'a> {
    callee: &'a str,
    first: &'a str,
    second: &'a str,
    member_base: &'a str,
    member_offset: u32,
}

fn classify(function: &Function) -> Option<AggregateReturnForwarder<'_>> {
    if !matches!(function.return_type, Type::Struct { .. })
        || !function.locals.is_empty()
        || !function.statements.is_empty()
        || !function.guards.is_empty()
        || function.asm_body.is_some()
    {
        return None;
    }
    let Expression::Call {
        name: callee,
        arguments,
    } = function.return_expression.as_ref()?
    else {
        return None;
    };
    let [
        Expression::Variable(first),
        Expression::Variable(second),
        Expression::Member {
            base,
            offset: member_offset,
            member_type: Type::Struct { .. },
            index_stride: None,
        },
    ] = arguments.as_slice()
    else {
        return None;
    };
    let Expression::Variable(member_base) = base.as_ref() else {
        return None;
    };
    Some(AggregateReturnForwarder {
        callee,
        first,
        second,
        member_base,
        member_offset: *member_offset,
    })
}

impl Generator {
    pub(crate) fn try_aggregate_return_forwarder(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        let Some(plan) = classify(function) else {
            return Ok(false);
        };
        if self.behavior.frame_convention != FrameConvention::Predecrement
            || !matches!(
                self.call_return_types.get(plan.callee),
                Some(Type::Struct { .. })
            )
        {
            return Ok(false);
        }

        let general_register = |name: &str| {
            self.locations
                .get(name)
                .filter(|location| location.class == ValueClass::General)
                .map(|location| location.register)
                .ok_or_else(|| {
                    Diagnostic::error(format!(
                        "aggregate return forwarding value '{name}' has no general register"
                    ))
                })
        };
        let first = general_register(plan.first)?;
        let second = general_register(plan.second)?;
        let member_base = general_register(plan.member_base)?;
        let hidden_result = Eabi::FIRST_GENERAL_ARGUMENT;
        if (first, second, member_base) != (5, 6, 4) {
            return Ok(false);
        }

        self.output.pre_scheduled = true;
        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved = vec![31];
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            });
        self.output
            .instructions
            .push(Instruction::MoveFromLinkRegister { d: 0 });

        // r7 breaks the r4/r5/r6 argument cycle; r0 retains the second
        // argument while r6 becomes the trailing aggregate-reference address.
        // The legacy scheduler interleaves both frame stores into that copy
        // chain, so this whole-body owner emits the measured final order.
        self.output
            .instructions
            .push(Instruction::move_register(7, member_base));
        self.output
            .instructions
            .push(Instruction::move_register(4, first));
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        });
        self.output
            .instructions
            .push(Instruction::move_register(0, second));
        if plan.member_offset == 0 {
            self.output
                .instructions
                .push(Instruction::move_register(6, 7));
        } else {
            let offset = i16::try_from(plan.member_offset).map_err(|_| {
                Diagnostic::error("an aggregate return forwarding member offset is out of range")
            })?;
            self.output.instructions.push(Instruction::AddImmediate {
                d: 6,
                a: 7,
                immediate: offset,
            });
        }
        self.output.instructions.push(Instruction::StoreWord {
            s: 31,
            a: 1,
            offset: 12,
        });

        // MWCC gives the hidden result a nonvolatile home across the producing
        // call even though the callee also returns that address in r3.
        self.output
            .instructions
            .push(Instruction::move_register(31, hidden_result));
        self.output
            .instructions
            .push(Instruction::move_register(5, 0));
        self.record_relocation(RelocationKind::Rel24, plan.callee);
        self.output.instructions.push(Instruction::BranchAndLink {
            target: plan.callee.to_string(),
        });
        self.emit_epilogue_and_return();
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_hidden_result_forwarding_shape() {
        let function = Function {
            return_type: Type::Struct { size: 4, align: 4 },
            name: "wrapper".into(),
            is_static: false,
            is_weak: false,
            parameters: Vec::new(),
            locals: Vec::new(),
            statements: Vec::new(),
            guards: Vec::new(),
            return_expression: Some(Expression::Call {
                name: "callee".into(),
                arguments: vec![
                    Expression::Variable("a".into()),
                    Expression::Variable("b".into()),
                    Expression::Member {
                        base: Box::new(Expression::Variable("this".into())),
                        offset: 0,
                        member_type: Type::Struct { size: 16, align: 4 },
                        index_stride: None,
                    },
                ],
            }),
            section: None,
            preceded_by_asm: false,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: false,
            text_deferred: false,
            peephole_disabled: false,
        };

        let plan = classify(&function).expect("the wrapper uses hidden-result forwarding");
        assert_eq!(plan.callee, "callee");
        assert_eq!(plan.first, "a");
        assert_eq!(plan.second, "b");
        assert_eq!(plan.member_base, "this");
        assert_eq!(plan.member_offset, 0);
    }
}
