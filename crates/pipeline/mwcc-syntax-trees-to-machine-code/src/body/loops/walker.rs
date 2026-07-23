//! The pointer-walker call loop: a NULL-terminated function-pointer table walked and each entry
//! called. This is the C++ static ctor/dtor runner mwcc emits into every REL module's `_prolog`
//! and `_epilog` (`while (*p != 0) { (**p)(); p++; }`).

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Compose a whole-file-IPA caller from a verified pointer-walker callee and
    /// an optional verified one-call wrapper. The ordinary walker recognizer
    /// remains the single owner of the emitted schedule.
    pub(crate) fn try_ipa_inlined_pointer_walker(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !self.behavior.tail_call_optimization {
            return Ok(false);
        }
        let Some((walker, trailing_wrapper)) =
            self.inline_summaries.ipa_pointer_walker_caller(function)
        else {
            return Ok(false);
        };
        let local_name = "__mwcc_ipa_walker".to_string();
        let loop_statement = Statement::Loop {
            kind: LoopKind::For,
            initializer: Some(Expression::Assign {
                target: Box::new(Expression::Variable(local_name.clone())),
                value: Box::new(Expression::Variable(walker.table.clone())),
            }),
            condition: Some(Expression::Dereference {
                pointer: Box::new(Expression::Variable(local_name.clone())),
            }),
            step: Some(Expression::Assign {
                target: Box::new(Expression::Variable(local_name.clone())),
                value: Box::new(Expression::Binary {
                    operator: BinaryOperator::Add,
                    left: Box::new(Expression::Variable(local_name.clone())),
                    right: Box::new(Expression::IntegerLiteral(1)),
                }),
            }),
            body: vec![Statement::Expression(Expression::Call {
                name: local_name.clone(),
                arguments: Vec::new(),
            })],
        };
        let mut statements = vec![loop_statement];
        if let Some(wrapper) = trailing_wrapper {
            statements.push(Statement::Expression(Expression::Call {
                name: wrapper.callee.clone(),
                arguments: wrapper.arguments.clone(),
            }));
        }
        let composed = Function {
            return_type: Type::Void,
            name: function.name.clone(),
            is_static: function.is_static,
            is_weak: function.is_weak,
            parameters: Vec::new(),
            locals: vec![LocalDeclaration {
                declared_type: Type::Pointer(Pointee::Pointer),
                name: local_name,
                initializer: None,
                is_volatile: false,
                array_length: None,
                is_static: false,
                data_bytes: None,
                data_relocations: Vec::new(),
                is_const: false,
                row_bytes: None,
            }],
            statements,
            guards: Vec::new(),
            return_expression: None,
            section: function.section.clone(),
            preceded_by_asm: function.preceded_by_asm,
            asm_body: None,
            inline_asm_blocks: Vec::new(),
            force_active: function.force_active,
            text_deferred: function.text_deferred,
            peephole_disabled: function.peephole_disabled,
        };
        self.try_pointer_walker_call_loop(&composed)
    }

    /// `_prolog`/`_epilog`: a local pointer walks a NULL-terminated global function-pointer table,
    /// calling each entry. `const VoidFunc* p = _ctors; while (*p != 0) { (**p)(); p++; } … return 0;`
    ///
    /// ```text
    ///   stwu; mflr; lis r3,tbl@ha; stw r0,20; addi r0,r3,tbl@lo; stw r31,12; mr r31,r0
    ///   b .cond
    ///   .body: mtctr r12; bctrl; addi r31,r31,4
    ///   .cond: lwz r12,0(r31); cmplwi r12,0; bne .body
    ///   [bl trailing…]; lwz r0,20; [li r3,0;] lwz r31,12; mtlr; addi r1,r1,16; blr
    /// ```
    ///
    /// The keystone is the r12 reuse: the condition load `lwz r12,0(r31)` (the next table entry)
    /// doubles as the indirect callee, so the body is just `mtctr r12; bctrl`. The walker lives in
    /// r31 (callee-saved, it crosses the calls). A `void` function (`_epilog`) drops the trailing
    /// call and the `li r3,0`; an `int` function returns 0. A retained static helper whose entire
    /// body is one call may be expanded through the TU's semantic inline summary. Any other trailing
    /// statement, a non-4-byte table element, or a differently-shaped loop defers.
    pub(crate) fn try_pointer_walker_call_loop(
        &mut self,
        function: &Function,
    ) -> Compilation<bool> {
        if !function.guards.is_empty()
            || !self.frame_slots.is_empty()
            || !function.parameters.is_empty()
        {
            return Ok(false);
        }
        let returns_int = match function.return_type {
            Type::Void => false,
            Type::Int => true,
            _ => return Ok(false),
        };
        // Exactly one local: a pointer initialized to a global (function-pointer) array's address.
        // Older runtime sources spell the same initialization in a `for` header instead of the
        // declaration, so the table is resolved after the loop shape is known below.
        let [local] = function.locals.as_slice() else {
            return Ok(false);
        };
        if local.array_length.is_some() || local.is_static {
            return Ok(false);
        }

        // The first statement is the walk loop; the rest are the trailing calls (and, for an `int`
        // function, a `return 0`).
        let Some((
            Statement::Loop {
                kind,
                initializer,
                condition: Some(condition),
                step,
                body,
            },
            trailing,
        )) = function.statements.split_first()
        else {
            return Ok(false);
        };
        // Normalize the two source spellings:
        //
        //   T *p = table; while (*p != 0) { (**p)(); p++; }
        //   T *p; for (p = table; *p; p++) { (*p)(); }
        //
        // The second is emitted by the older C++ runtime with peephole-disabled label branches.
        let for_header_form = *kind == LoopKind::For;
        // The old Runtime `for` spelling spans both frame families. Linkage-first
        // builds select between retained source edges and an interleaved address
        // schedule; later predecrement builds use the canonical r31/CTR walker.
        let table = match (kind, &local.initializer, initializer) {
            (LoopKind::While, Some(Expression::Variable(table)), None) if step.is_none() => {
                table.clone()
            }
            (LoopKind::For, None, Some(Expression::Assign { target, value }))
                if matches!(target.as_ref(), Expression::Variable(name) if name == &local.name)
                    && matches!(value.as_ref(), Expression::Variable(_)) =>
            {
                let Expression::Variable(table) = value.as_ref() else {
                    unreachable!()
                };
                table.clone()
            }
            _ => return Ok(false),
        };
        // The table is a global function-pointer array (often an unsized `extern T[]`, so it is not
        // in `global_array_sizes`); its address is taken with an ADDR16 relocation regardless. Only
        // a global symbol qualifies — a local/parameter name would not carry the relocation.
        if self.locations.contains_key(table.as_str()) {
            return Ok(false);
        }
        // The condition is `*p != 0` or its truth-test shorthand `*p` — the table entry loaded and
        // tested against null.
        let walks_local = |expression: &Expression| {
            matches!(expression,
            Expression::Dereference { pointer } if matches!(pointer.as_ref(), Expression::Variable(name) if *name == local.name))
        };
        match condition {
            Expression::Binary {
                operator: BinaryOperator::NotEqual,
                left,
                right,
            } if walks_local(left) && matches!(right.as_ref(), Expression::IntegerLiteral(0)) => {}
            expression if walks_local(expression) => {}
            _ => return Ok(false),
        }
        // The call is represented either as CallThrough(Dereference(p)) for `(**p)()` or as a
        // named call to the pointer local for `(*p)()`. Both consume the r12 value loaded by the
        // condition. The pointer update lives in the while body or the for-loop step field.
        let (call_statement, step_expression) = if for_header_form {
            let [call] = body.as_slice() else {
                return Ok(false);
            };
            let Some(step) = step else {
                return Ok(false);
            };
            (call, step)
        } else {
            let [call, Statement::Assign { name, value }] = body.as_slice() else {
                return Ok(false);
            };
            if name != &local.name {
                return Ok(false);
            }
            (call, value)
        };
        let call_matches = match call_statement {
            Statement::Expression(Expression::CallThrough { target, arguments }) => {
                arguments.is_empty() && walks_local(target)
            }
            Statement::Expression(Expression::Call { name, arguments }) => {
                arguments.is_empty() && name == &local.name
            }
            _ => false,
        };
        if !call_matches {
            return Ok(false);
        }
        match step_expression {
            Expression::Assign { target, value }
                if for_header_form
                    && matches!(target.as_ref(), Expression::Variable(name) if name == &local.name)
                    && matches!(value.as_ref(), Expression::Binary {
                        operator: BinaryOperator::Add,
                        left,
                        right,
                    } if matches!(left.as_ref(), Expression::Variable(other) if other == &local.name)
                        && matches!(right.as_ref(), Expression::IntegerLiteral(1))) => {}
            Expression::Binary {
                operator: BinaryOperator::Add,
                left,
                right,
            } if !for_header_form
                && matches!(left.as_ref(), Expression::Variable(other) if other == &local.name)
                && matches!(right.as_ref(), Expression::IntegerLiteral(1)) => {}
            _ => return Ok(false),
        }

        // The trailing statements: zero or more bare calls, then (for `int`) an optional `return 0`.
        let mut trailing_calls: Vec<(
            String,
            Option<crate::inline_summaries::StaticCallWrapperSummary>,
        )> = Vec::new();
        let mut saw_return_zero = false;
        for statement in trailing {
            match statement {
                Statement::Expression(Expression::Call { name, arguments })
                    if arguments.is_empty() =>
                {
                    let wrapper = self.inline_summaries.static_call_wrapper(name).cloned();
                    trailing_calls.push((name.clone(), wrapper));
                }
                Statement::Return(Some(Expression::IntegerLiteral(0))) if returns_int => {
                    saw_return_zero = true;
                }
                _ => return Ok(false),
            }
        }
        // A trailing call must be a genuine external (a file-scope prototype), or a verified
        // parameterless static wrapper. The latter stays emitted out of line but its one-call body
        // is expanded here (`ObjectSetup()` -> `BoardObjectSetup(BoardCreate, BoardDestroy)`).
        if trailing_calls
            .iter()
            .any(|(name, wrapper)| !self.prototyped_names.contains(name) && wrapper.is_none())
        {
            return Ok(false);
        }
        let returns_zero = returns_int
            && (saw_return_zero
                || matches!(
                    function.return_expression.as_ref(),
                    Some(Expression::IntegerLiteral(0))
                ));
        if returns_int && !returns_zero {
            return Ok(false);
        }
        if !returns_int && function.return_expression.is_some() {
            return Ok(false);
        }

        const WALKER: u8 = 31;
        // The table entry is a function pointer (4 bytes); the walk steps by that.
        let step = 4i16;
        self.non_leaf = true;
        self.frame_size = 16;
        self.callee_saved = vec![WALKER];
        // Peepholes can remove redundant branch edges without rolling back the source
        // `for` accounting. A preceding asm definition retains the edges through a
        // different file-level path and consumes one fewer anonymous slot.
        self.output.anonymous_label_bump = if for_header_form && !function.preceded_by_asm {
            6
        } else {
            5
        };
        // This recognizer owns the complete function schedule for every optimization level.
        self.output.pre_scheduled = true;

        let style = self.behavior.pointer_walker_schedule_style;
        let legacy_for_schedule =
            for_header_form && self.behavior.frame_convention == FrameConvention::LinkageFirst;
        let modern_for_schedule = for_header_form
            && self.behavior.frame_convention == FrameConvention::Predecrement
            && style == PointerWalkerScheduleStyle::LatencyInterleaved;
        let signed_for_condition =
            for_header_form && self.behavior.frame_convention == FrameConvention::Predecrement;
        // A preceding asm definition suppresses the same file-level optimization as
        // explicit `#pragma peephole off`, retaining the source `for` edges.
        let legacy_for_keeps_edges =
            legacy_for_schedule && (function.preceded_by_asm || function.peephole_disabled);
        let optimized_legacy_for_schedule = legacy_for_schedule && !legacy_for_keeps_edges;
        let direct_address =
            modern_for_schedule || style == PointerWalkerScheduleStyle::DirectAddressDuplicateLoad;
        let duplicate_entry_load = !modern_for_schedule
            && matches!(
                style,
                PointerWalkerScheduleStyle::DirectAddressDuplicateLoad
                    | PointerWalkerScheduleStyle::ScratchAddressDuplicateLoad
            );
        let interleave_linkage =
            !modern_for_schedule && style == PointerWalkerScheduleStyle::LatencyInterleaved;

        // O0..O3 retain the canonical linkage saves before table materialization. O4 moves `lis`
        // into the mflr latency slot and the dependent scratch `addi` between the two saves.
        if legacy_for_keeps_edges {
            self.output
                .instructions
                .push(Instruction::MoveFromLinkRegister { d: 0 });
            self.output.instructions.push(Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            });
            self.output
                .instructions
                .push(Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -16,
                });
            self.output.instructions.push(Instruction::StoreWord {
                s: WALKER,
                a: 1,
                offset: 12,
            });
        } else if optimized_legacy_for_schedule {
            self.output
                .instructions
                .push(Instruction::MoveFromLinkRegister { d: 0 });
        } else {
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
            if !interleave_linkage {
                self.output.instructions.push(Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: 20,
                });
                self.output.instructions.push(Instruction::StoreWord {
                    s: WALKER,
                    a: 1,
                    offset: 12,
                });
            }
        }
        self.record_relocation(RelocationKind::Addr16Ha, &table);
        self.output
            .instructions
            .push(Instruction::AddImmediateShifted {
                d: if modern_for_schedule { WALKER } else { 3 },
                a: 0,
                immediate: 0,
            });
        if optimized_legacy_for_schedule {
            self.output.instructions.push(Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 4,
            });
        }
        if interleave_linkage && !legacy_for_schedule {
            self.output.instructions.push(Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 20,
            });
        }
        self.record_relocation(RelocationKind::Addr16Lo, &table);
        self.output.instructions.push(Instruction::AddImmediate {
            d: if direct_address && !legacy_for_schedule {
                WALKER
            } else {
                0
            },
            a: if modern_for_schedule { WALKER } else { 3 },
            immediate: 0,
        });
        if optimized_legacy_for_schedule {
            self.output
                .instructions
                .push(Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -16,
                });
            self.output.instructions.push(Instruction::StoreWord {
                s: WALKER,
                a: 1,
                offset: 12,
            });
        }
        if interleave_linkage && !legacy_for_schedule {
            self.output.instructions.push(Instruction::StoreWord {
                s: WALKER,
                a: 1,
                offset: 12,
            });
        }
        if (!direct_address || legacy_for_schedule) && !modern_for_schedule {
            self.output.instructions.push(Instruction::Or {
                a: WALKER,
                s: 0,
                b: 0,
            });
        }

        // At O0/O1 the source-level body and condition each load `*walker`. O2+ eliminates the body
        // load and reuses the condition's r12 value as the next indirect callee.
        if legacy_for_keeps_edges {
            let next = self.output.instructions.len() + 1;
            self.output
                .instructions
                .push(Instruction::Branch { target: next });
            let next = self.output.instructions.len() + 1;
            self.output
                .instructions
                .push(Instruction::Branch { target: next });
        }
        let skip_to_condition = self.output.instructions.len();
        self.output
            .instructions
            .push(Instruction::Branch { target: 0 });
        let body_top = self.output.instructions.len();
        if duplicate_entry_load && !legacy_for_schedule {
            self.output.instructions.push(Instruction::LoadWord {
                d: 12,
                a: WALKER,
                offset: 0,
            });
        }
        if legacy_for_schedule {
            self.output
                .instructions
                .push(Instruction::MoveToLinkRegister { s: 12 });
            self.output
                .instructions
                .push(Instruction::BranchToLinkRegisterAndLink);
        } else {
            self.output
                .instructions
                .push(Instruction::MoveToCountRegister { s: 12 });
            self.output
                .instructions
                .push(Instruction::BranchToCountRegisterAndLink);
        }
        self.output.instructions.push(Instruction::AddImmediate {
            d: WALKER,
            a: WALKER,
            immediate: step,
        });
        let condition_top = self.output.instructions.len();
        if let Instruction::Branch { target } = &mut self.output.instructions[skip_to_condition] {
            *target = condition_top;
        }
        let condition_register = if duplicate_entry_load && !legacy_for_schedule {
            0
        } else {
            12
        };
        self.output.instructions.push(Instruction::LoadWord {
            d: condition_register,
            a: WALKER,
            offset: 0,
        });
        self.output.instructions.push(if signed_for_condition {
            Instruction::CompareWordImmediate {
                a: condition_register,
                immediate: 0,
            }
        } else {
            Instruction::CompareLogicalWordImmediate {
                a: condition_register,
                immediate: 0,
            }
        });
        self.output
            .instructions
            .push(Instruction::BranchConditionalForward {
                options: 4,
                condition_bit: 2,
                target: body_top,
            });

        // The trailing calls, then the final schedule. O4 issues the saved-LR reload early to overlap
        // it with return-value materialization and the GPR reload; lower levels retain source/canonical
        // `li; lwz r31; lwz r0` order.
        for (name, wrapper) in &trailing_calls {
            if let Some(wrapper) = wrapper {
                self.emit_call(&wrapper.callee, &wrapper.arguments, None, false)?;
            } else {
                self.record_relocation(RelocationKind::Rel24, name);
                self.output.instructions.push(Instruction::BranchAndLink {
                    target: name.clone(),
                });
            }
        }
        if interleave_linkage || legacy_for_schedule || modern_for_schedule {
            self.output.instructions.push(Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: self.frame_size + 4,
            });
        }
        if returns_zero {
            self.output.instructions.push(Instruction::AddImmediate {
                d: 3,
                a: 0,
                immediate: 0,
            });
        }
        self.output.instructions.push(Instruction::LoadWord {
            d: WALKER,
            a: 1,
            offset: self.frame_size - 4,
        });
        if !interleave_linkage && !legacy_for_schedule && !modern_for_schedule {
            self.output.instructions.push(Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: self.frame_size + 4,
            });
        }
        if legacy_for_schedule {
            self.output.instructions.push(Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: self.frame_size,
            });
            self.output
                .instructions
                .push(Instruction::MoveToLinkRegister { s: 0 });
        } else {
            self.output
                .instructions
                .push(Instruction::MoveToLinkRegister { s: 0 });
            self.output.instructions.push(Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: self.frame_size,
            });
        }
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
        Ok(true)
    }
}
