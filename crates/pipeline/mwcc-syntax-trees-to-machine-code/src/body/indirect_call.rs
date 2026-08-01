//! Bare indirect-call statements through a memory-resident function pointer.
//!
//! This module owns both the general statement form and the measured whole-function schedule
//! for constant arguments. Keeping the callee staging and argument-safety rules together avoids
//! teaching the statement driver about indirect-call register dependencies.

#[allow(unused_imports)]
use super::*;

mod guarded_indexed;
mod guarded_shared_global;
mod global_member_callback;
mod indexed_frame;
mod indexed_mixed_arguments;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ArgumentPlacement {
    Register { source: u8, target: u8 },
    Constant { value: i64, target: u8 },
    FunctionAddress { target: u8 },
}

fn placement_overwrites_later_source(placements: &[ArgumentPlacement]) -> bool {
    placements
        .iter()
        .enumerate()
        .any(|(index, placement)| {
            let target = match placement {
                ArgumentPlacement::Register { source, target } if source != target => *target,
                ArgumentPlacement::Constant { target, .. }
                | ArgumentPlacement::FunctionAddress { target } => *target,
                ArgumentPlacement::Register { .. } => return false,
            };
            placements[index + 1..].iter().any(|later| {
                matches!(
                    later,
                    ArgumentPlacement::Register { source, .. } if *source == target
                )
            })
        })
}

impl Generator {
    /// `global_object->callback(global_object)`: materialize the shared global
    /// pointer once, stage its callback in r12, and pass the same pointer in r3.
    ///
    /// A guarded call can arrive with the pointer already carried from its
    /// condition. Keep that virtual home as the load base until the callee has
    /// been staged, then copy it into the first ABI argument.
    fn try_emit_shared_global_base_indirect_call(
        &mut self,
        target: &Expression,
        arguments: &[Expression],
    ) -> Compilation<bool> {
        let Some((global, offset)) = shared_global_base_member_call(target, arguments) else {
            return Ok(false);
        };
        if self.locations.contains_key(global)
            || !matches!(
                self.globals.get(global),
                Some(Type::StructPointer { .. })
            )
        {
            return Ok(false);
        }
        let offset = i16::try_from(offset)
            .map_err(|_| Diagnostic::error("indirect callback member offset is out of range"))?;
        let base = if let Some(base) = self.condition_global_base(global)? {
            base
        } else {
            self.emit_global_load_value(global, Eabi::FIRST_GENERAL_ARGUMENT)?;
            Eabi::FIRST_GENERAL_ARGUMENT
        };
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: base,
            offset,
        });
        if base != Eabi::FIRST_GENERAL_ARGUMENT {
            self.emit_integer_materialization_copy(Eabi::FIRST_GENERAL_ARGUMENT, base);
        }
        self.emit_indirect_branch_and_link(12);
        Ok(true)
    }

    /// `object->callback(constant, object)`: preserve the shared base in the
    /// second ABI argument before the first argument overwrites r3. The callee
    /// is staged first so a guarded callback can later reuse its null-test load.
    fn try_emit_shared_base_constant_indirect_call(
        &mut self,
        target: &Expression,
        arguments: &[Expression],
    ) -> Compilation<bool> {
        let Expression::Member {
            base: target_base,
            ..
        } = target
        else {
            return Ok(false);
        };
        let Expression::Variable(base_name) = target_base.as_ref() else {
            return Ok(false);
        };
        let [Expression::IntegerLiteral(constant), Expression::Variable(second)] = arguments else {
            return Ok(false);
        };
        if second != base_name {
            return Ok(false);
        }
        let base = self.general_register_of(base_name)?;
        if base == 12 {
            return Ok(false);
        }

        // A short-lived local used as the second argument can live in r4 from
        // its definition. The explicit copy below then coalesces as a self
        // move; a fixed r3 call result still retains the required copy.
        self.prefer_virtual_general_if_unset(base, Eabi::FIRST_GENERAL_ARGUMENT + 1);
        self.evaluate(target, Type::UnsignedInt, 12)?;
        if base != Eabi::FIRST_GENERAL_ARGUMENT + 1 {
            self.emit_integer_materialization_copy(
                Eabi::FIRST_GENERAL_ARGUMENT + 1,
                base,
            );
        }
        self.load_integer_constant(Eabi::FIRST_GENERAL_ARGUMENT, *constant);
        self.emit_indirect_branch_and_link(12);
        Ok(true)
    }

    /// `object->callback(object->value, object)`: preserve the shared base in
    /// the second ABI argument before the first argument overwrites r3. The
    /// callee load remains ahead of the computed member argument, matching the
    /// callback schedule used inside structured DVD state handlers.
    fn try_emit_shared_base_member_indirect_call(
        &mut self,
        target: &Expression,
        arguments: &[Expression],
    ) -> Compilation<bool> {
        let Expression::Member {
            base: target_base,
            ..
        } = target
        else {
            return Ok(false);
        };
        let Expression::Variable(base_name) = target_base.as_ref() else {
            return Ok(false);
        };
        let [first, Expression::Variable(second)] = arguments else {
            return Ok(false);
        };
        let first_member = match first {
            member @ Expression::Member { .. } => member,
            Expression::Cast {
                target_type,
                operand,
            } if target_type.width() == 32
                && matches!(
                    operand.as_ref(),
                    Expression::Member { member_type, .. } if member_type.width() == 32
                ) =>
            {
                operand.as_ref()
            }
            _ => return Ok(false),
        };
        let Expression::Member {
            base: first_base, ..
        } = first_member
        else {
            return Ok(false);
        };
        if second != base_name
            || !matches!(first_base.as_ref(), Expression::Variable(name) if name == base_name)
        {
            return Ok(false);
        }
        let base = self.general_register_of(base_name)?;
        if base == Eabi::FIRST_GENERAL_ARGUMENT {
            self.output.instructions.push(Instruction::move_register(
                Eabi::FIRST_GENERAL_ARGUMENT + 1,
                base,
            ));
            self.evaluate(target, Type::UnsignedInt, 12)?;
            self.evaluate_general(first, Eabi::FIRST_GENERAL_ARGUMENT)?;
        } else {
            self.evaluate(target, Type::UnsignedInt, 12)?;
            self.evaluate_general(first, Eabi::FIRST_GENERAL_ARGUMENT)?;
            if base != Eabi::FIRST_GENERAL_ARGUMENT + 1 {
                self.output.instructions.push(Instruction::move_register(
                    Eabi::FIRST_GENERAL_ARGUMENT + 1,
                    base,
                ));
            }
        }
        self.emit_indirect_branch_and_link(12);
        Ok(true)
    }

    fn indirect_argument_placements(
        &self,
        arguments: &[Expression],
    ) -> Compilation<Vec<ArgumentPlacement>> {
        let placements = arguments
            .iter()
            .enumerate()
            .map(|(index, argument)| {
                let target = Eabi::FIRST_GENERAL_ARGUMENT + index as u8;
                if let Expression::IntegerLiteral(value) = argument {
                    return Ok(ArgumentPlacement::Constant {
                        value: *value,
                        target,
                    });
                }
                if let Expression::Variable(name) = argument {
                    if self.is_direct_function_symbol(name) {
                        return Ok(ArgumentPlacement::FunctionAddress { target });
                    }
                }
                let (source, width, _) = self.leaf_info(argument)?;
                if width != 32 || source == 12 {
                    return Err(Diagnostic::error(
                        "arguments to a bare indirect call need dependency-aware marshaling (roadmap)",
                    ));
                }
                Ok(ArgumentPlacement::Register { source, target })
            })
            .collect::<Compilation<Vec<_>>>()?;
        if arguments.len() > 8 || placement_overwrites_later_source(&placements) {
            return Err(Diagnostic::error(
                "arguments to a bare indirect call need dependency-aware marshaling (roadmap)",
            ));
        }
        Ok(placements)
    }

    fn emit_indirect_arguments(
        &mut self,
        arguments: &[Expression],
        placements: &[ArgumentPlacement],
    ) -> Compilation<()> {
        for (argument, placement) in arguments.iter().zip(placements) {
            match *placement {
                ArgumentPlacement::Register { source, target } if source != target => {
                    self.evaluate_general(argument, target)?;
                }
                ArgumentPlacement::Constant { value, target } => {
                    self.load_integer_constant(target, value);
                }
                ArgumentPlacement::FunctionAddress { target } => {
                    let Expression::Variable(name) = argument else {
                        unreachable!("a function-address placement came from a designator")
                    };
                    self.emit_function_address_value(name, target);
                }
                ArgumentPlacement::Register { .. } => {}
            }
        }
        Ok(())
    }

    /// Emit a bare indirect-call statement such as `actor->proc(actor)`.
    ///
    /// The callee is staged in r12 before the call. Arguments are currently accepted only when
    /// every one is a word-sized general-register leaf and the left-to-right moves are acyclic:
    /// no destination may still hold a later argument. This covers both pure pass-through calls
    /// and the common `saved_actor->proc(saved_actor)` tail while ensuring argument marshaling
    /// cannot destroy either the callee or a later argument. Cyclic moves and computed arguments
    /// keep deferring until their schedules can be modeled explicitly.
    pub(crate) fn emit_bare_indirect_call_statement(
        &mut self,
        target: &Expression,
        arguments: &[Expression],
    ) -> Compilation<()> {
        if self.try_emit_global_member_callback_indirect_call(target, arguments)? {
            return Ok(());
        }
        if self.try_emit_frame_indexed_global_indirect_call(target, arguments)? {
            return Ok(());
        }
        if self.try_emit_shared_global_base_indirect_call(target, arguments)? {
            return Ok(());
        }
        if self.try_emit_shared_base_constant_indirect_call(target, arguments)? {
            return Ok(());
        }
        if self.try_emit_shared_base_member_indirect_call(target, arguments)? {
            return Ok(());
        }
        if self.try_emit_indexed_indirect_call_with_mixed_arguments(
            target,
            arguments,
        )? {
            return Ok(());
        }
        if !matches!(
            target,
            Expression::Dereference { .. } | Expression::Member { .. }
        ) {
            return Err(Diagnostic::error(
                "this bare indirect-call target is not supported yet (roadmap)",
            ));
        }
        let placements = self.indirect_argument_placements(arguments)?;

        self.evaluate(target, Type::UnsignedInt, 12)?;
        self.emit_indirect_arguments(arguments, &placements)?;
        self.emit_indirect_branch_and_link(12);
        Ok(())
    }

    /// A bare indirect call through a MEMORY-resident function pointer, passing small integer
    /// constants: `void f(struct S *s){ s->cb(7); }` / `void f(VF *pp){ (**pp)(7); }`. The callee
    /// address lives at `off(param)`; its base sits in r3, colliding with the first argument, so
    /// mwcc copies the base to r4, materializes the first argument (`li r3,c0`), saves the link
    /// register (latency-filled into the mflr gap), loads the callee (`lwz r12,off(r4)`), then
    /// materializes the remaining arguments (r4 is free again) and `mtctr r12; bctrl`:
    ///
    /// ```text
    ///   stwu; mflr r0; mr r4,r3; li r3,c0; stw r0,20; lwz r12,off(r4); li r4,c1; …; mtctr; bctrl
    /// ```
    ///
    /// Only a single pointer parameter as the base and all-constant arguments are modeled; a
    /// computed/parameter argument, a non-parameter base, or a returned result defers (unmeasured).
    pub(crate) fn try_indirect_call_with_constant_args(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if function.return_type != Type::Void
            || function.return_expression.is_some()
            || !function.guards.is_empty()
            || !function.locals.is_empty()
            || !self.frame_slots.is_empty()
            || function.parameters.len() != 1
        {
            return Ok(false);
        }
        // The body is exactly one bare indirect call.
        let [Statement::Expression(Expression::CallThrough { target, arguments })] =
            function.statements.as_slice()
        else {
            return Ok(false);
        };
        if arguments.is_empty() || arguments.len() > 8 {
            return Ok(false);
        }
        // The callee address is `off(param)`: either `*param` (offset 0) or `param->member`.
        let parameter_name = &function.parameters[0].name;
        let offset = match target.as_ref() {
            Expression::Dereference { pointer } => match pointer.as_ref() {
                Expression::Variable(name) if name == parameter_name => 0i16,
                _ => return Ok(false),
            },
            Expression::Member { base, offset, .. } => match base.as_ref() {
                Expression::Variable(name) if name == parameter_name => *offset as i16,
                _ => return Ok(false),
            },
            _ => return Ok(false),
        };
        // Every argument is a small integer constant.
        let mut constants = Vec::with_capacity(arguments.len());
        for argument in arguments {
            match argument {
                Expression::IntegerLiteral(value)
                    if (i16::MIN as i64..=i16::MAX as i64).contains(value) =>
                {
                    constants.push(*value as i16);
                }
                _ => return Ok(false),
            }
        }
        // Sanity: the base parameter arrives in r3 (a general register).
        match self.locations.get(parameter_name) {
            Some(location) if location.class == ValueClass::General && location.register == 3 => {}
            _ => return Ok(false),
        }

        // The base register r3 collides with the first argument, so it is copied to r4 and the
        // callee is loaded from there AFTER the first argument and the link-register save (which
        // fills the mflr latency gap). Emitting this pre-scheduled keeps the passes off it: `mflr`
        // is not immediately followed by the save, so the link-register scheduler leaves it be.
        self.non_leaf = true;
        self.frame_size = 16;
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
        self.output
            .instructions
            .push(Instruction::move_register(4, 3)); // mr r4,r3
        self.output.instructions.push(Instruction::AddImmediate {
            d: 3,
            a: 0,
            immediate: constants[0],
        }); // li r3,c0
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: 20,
        }); // stw r0,20
        self.output.instructions.push(Instruction::LoadWord {
            d: 12,
            a: 4,
            offset,
        }); // lwz r12,off(r4)
        for (index, &value) in constants.iter().enumerate().skip(1) {
            self.output.instructions.push(Instruction::AddImmediate {
                d: 3 + index as u8,
                a: 0,
                immediate: value,
            });
        }
        self.output
            .instructions
            .push(Instruction::MoveToCountRegister { s: 12 });
        self.output
            .instructions
            .push(Instruction::BranchToCountRegisterAndLink);
        self.emit_epilogue_and_return();
        Ok(true)
    }
}

fn shared_global_base_member_call<'a>(
    target: &'a Expression,
    arguments: &'a [Expression],
) -> Option<(&'a str, u32)> {
    let Expression::Member {
        base,
        offset,
        member_type: Type::Pointer(_) | Type::StructPointer { .. },
        index_stride: None,
    } = target
    else {
        return None;
    };
    let (
        Expression::Variable(base),
        [Expression::Variable(argument)],
    ) = (base.as_ref(), arguments)
    else {
        return None;
    };
    (base == argument).then_some((base.as_str(), *offset))
}

#[cfg(test)]
mod tests {
    use super::{
        placement_overwrites_later_source, shared_global_base_member_call,
        ArgumentPlacement,
    };
    use mwcc_syntax_trees::{Expression, Pointee, Type};

    #[test]
    fn accepts_a_constant_before_an_independent_register_argument() {
        let placements = [
            ArgumentPlacement::Constant {
                value: 0,
                target: 3,
            },
            ArgumentPlacement::Register {
                source: 4,
                target: 4,
            },
        ];

        assert!(!placement_overwrites_later_source(&placements));
    }

    #[test]
    fn rejects_a_constant_that_overwrites_a_later_argument_source() {
        let placements = [
            ArgumentPlacement::Constant {
                value: 0,
                target: 3,
            },
            ArgumentPlacement::Register {
                source: 3,
                target: 4,
            },
        ];

        assert!(placement_overwrites_later_source(&placements));
    }

    #[test]
    fn recognizes_a_member_callback_and_argument_with_one_shared_global_base() {
        let target = Expression::Member {
            base: Box::new(Expression::Variable("current".into())),
            offset: 40,
            member_type: Type::Pointer(Pointee::UnsignedInt),
            index_stride: None,
        };
        let arguments = [Expression::Variable("current".into())];

        assert_eq!(
            shared_global_base_member_call(&target, &arguments),
            Some(("current", 40))
        );
    }

    #[test]
    fn rejects_a_member_callback_with_a_distinct_argument_base() {
        let target = Expression::Member {
            base: Box::new(Expression::Variable("current".into())),
            offset: 40,
            member_type: Type::Pointer(Pointee::UnsignedInt),
            index_stride: None,
        };
        let arguments = [Expression::Variable("other".into())];

        assert_eq!(shared_global_base_member_call(&target, &arguments), None);
    }
}
