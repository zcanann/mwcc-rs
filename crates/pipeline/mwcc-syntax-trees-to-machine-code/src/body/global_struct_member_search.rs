//! Bounded scans over a file-scope array of structures.
//!
//! CARD-style callback dispatchers recover the owning record by comparing the
//! address of one embedded member against an incoming pointer.  MWCC strength
//! reduces the array subscript to a byte displacement and uses CTR for the
//! compile-time bound; keeping that policy here avoids teaching the ordinary
//! expression emitter about loop topology.

#[allow(unused_imports)]
use super::*;

struct GlobalStructMemberSearch<'a> {
    counter: &'a str,
    result: &'a str,
    global: &'a str,
    needle: &'a str,
    bound: i16,
    stride: i16,
    member_offset: i16,
}

fn variable(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Variable(name) => Some(name),
        _ => None,
    }
}

fn assigned_zero(expression: &Expression) -> Option<&str> {
    match expression {
        Expression::Assign { target, value } if constant_value(value) == Some(0) => {
            variable(target)
        }
        _ => None,
    }
}

fn incremented_by_one(expression: &Expression, expected: &str) -> bool {
    matches!(expression,
        Expression::Assign { target, value }
            if variable(target) == Some(expected)
                && matches!(value.as_ref(), Expression::Binary {
                    operator: BinaryOperator::Add,
                    left,
                    right,
                } if variable(left) == Some(expected) && constant_value(right) == Some(1)))
}

fn classify<'a>(
    statement: &'a Statement,
    global_array_sizes: &std::collections::HashMap<String, u32>,
) -> Option<GlobalStructMemberSearch<'a>> {
    let Statement::Loop {
        kind: LoopKind::For,
        initializer: Some(initializer),
        condition: Some(condition),
        step: Some(step),
        body,
    } = statement
    else {
        return None;
    };
    let counter = assigned_zero(initializer)?;
    let bound = match condition {
        Expression::Binary {
            operator: BinaryOperator::Less,
            left,
            right,
        } if variable(left) == Some(counter) => i16::try_from(constant_value(right)?).ok()?,
        _ => return None,
    };
    if bound <= 0 || !incremented_by_one(step, counter) {
        return None;
    }

    let [Statement::Assign {
        name: result,
        value: Expression::AddressOf { operand: indexed },
    }, Statement::If {
        condition,
        then_body,
        else_body,
    }] = body.as_slice()
    else {
        return None;
    };
    let Expression::Index { base, index } = indexed.as_ref() else {
        return None;
    };
    let global = variable(base)?;
    if variable(index) != Some(counter) {
        return None;
    }
    let total_bytes = *global_array_sizes.get(global)?;
    let bound_u32 = u32::try_from(bound).ok()?;
    if total_bytes % bound_u32 != 0 {
        return None;
    }
    let stride = i16::try_from(total_bytes / bound_u32).ok()?;
    if stride <= 0 || !else_body.is_empty() || !matches!(then_body.as_slice(), [Statement::Break]) {
        return None;
    }

    let Expression::Binary {
        operator: BinaryOperator::Equal,
        left,
        right,
    } = condition
    else {
        return None;
    };
    let member_offset = match left.as_ref() {
        Expression::AddressOf { operand } => match operand.as_ref() {
            Expression::Member {
                base,
                offset,
                index_stride: None,
                ..
            } if variable(base) == Some(result) => i16::try_from(*offset).ok()?,
            _ => return None,
        },
        _ => return None,
    };
    let needle = variable(right)?;

    Some(GlobalStructMemberSearch {
        counter,
        result,
        global,
        needle,
        bound,
        stride,
        member_offset,
    })
}

pub(super) fn is_global_struct_member_search_loop(
    statement: &Statement,
    global_array_sizes: &std::collections::HashMap<String, u32>,
) -> bool {
    classify(statement, global_array_sizes).is_some()
}

pub(super) fn global_struct_member_search_result(statement: &Statement) -> Option<&str> {
    let Statement::Loop { body, .. } = statement else {
        return None;
    };
    match body.first() {
        Some(Statement::Assign { name, .. }) => Some(name),
        _ => None,
    }
}

impl Generator {
    /// Emit the CARD-style `for` scan if `statement` has the measured semantic
    /// shape. Returns false without changing generator state for every other loop.
    pub(crate) fn try_emit_global_struct_member_search_loop(
        &mut self,
        statement: &Statement,
    ) -> Compilation<bool> {
        self.try_emit_global_struct_member_search_loop_in_function(statement, None)
    }

    /// Frame-resident bodies retain source locals that are trivial aliases of
    /// parameters. Resolve that alias from the owning function without emitting
    /// a redundant move; MWCC compares against the incoming register directly.
    pub(crate) fn try_emit_global_struct_member_search_loop_in_function(
        &mut self,
        statement: &Statement,
        function: Option<&Function>,
    ) -> Compilation<bool> {
        let Some(search) = classify(statement, &self.global_array_sizes) else {
            return Ok(false);
        };
        let needle = self
            .lookup_general(search.needle)
            .or_else(|| {
                let local = function?
                    .locals
                    .iter()
                    .find(|local| local.name == search.needle)?;
                let mut initializer = local.initializer.as_ref()?;
                while let Expression::Cast { operand, .. } = initializer {
                    initializer = operand;
                }
                variable(initializer).and_then(|name| self.lookup_general(name))
            })
            .ok_or_else(|| {
                Diagnostic::error("global struct member search needle has no register")
            })?;

        // This measured schedule is specifically the first executable region
        // of a non-leaf function. Decline before allocating or mutating if a
        // preceding statement has already extended the three-instruction frame
        // prefix; a later-position scan needs its own independently measured
        // block-boundary schedule.
        let legacy_dense = self.behavior.frame_convention == FrameConvention::LinkageFirst
            && matches!(
                self.output.instructions.as_slice(),
                [
                    Instruction::MoveFromLinkRegister { d: 0 },
                    Instruction::StoreWord { s: 0, a: 1, .. },
                    Instruction::StoreWordWithUpdate { s: 1, a: 1, .. },
                    Instruction::StoreMultipleWord { a: 1, .. }
                ]
            );
        if !self.non_leaf || (self.output.instructions.len() != 3 && !legacy_dense) {
            return Ok(false);
        }
        let prefix_matches = match self.behavior.frame_convention {
            FrameConvention::Predecrement => matches!(
                self.output.instructions.last(),
                Some(Instruction::StoreWord { s: 0, a: 1, .. })
            ),
            FrameConvention::LinkageFirst if legacy_dense => true,
            FrameConvention::LinkageFirst => matches!(
                self.output.instructions.as_slice(),
                [
                    Instruction::MoveFromLinkRegister { d: 0 },
                    Instruction::StoreWord { s: 0, a: 1, .. },
                    Instruction::StoreWordWithUpdate { s: 1, a: 1, .. }
                ]
            ),
        };
        if !prefix_matches {
            return Ok(false);
        }

        // Preferences describe the measured volatile lane while still allowing
        // the allocator to move the scan around incoming/live values in a larger
        // function. The address high half dies when `base` is formed, so `record`
        // can legally recycle its preferred r4 home at the loop head.
        let search_result_is_keystone = function.is_some_and(|function| {
            function
                .statements
                .iter()
                .skip(1)
                .filter(|statement| statement_references_name(statement, search.result))
                .count()
                >= 6
        });
        let address_high = self.fresh_virtual_general_preferring(4);
        let base = self.fresh_virtual_general_preferring(if legacy_dense { 5 } else { 6 });
        let counter = self
            .lookup_general(search.counter)
            .unwrap_or_else(|| self.fresh_virtual_general_preferring(7));
        let displacement = self.fresh_virtual_general_preferring(if legacy_dense { 4 } else { 5 });
        let record_home = self.lookup_general(search.result);
        let record = if legacy_dense && !search_result_is_keystone {
            self.fresh_virtual_general_preferring(6)
        } else {
            record_home.unwrap_or_else(|| self.fresh_virtual_general_preferring(4))
        };

        // Both linkage conventions have emitted their three-instruction frame
        // prefix before statements begin. MWCC overlaps the invariant setup with
        // the final prefix instruction: LR store on mainline, stack allocation on
        // build 163. Delay that instruction here so branch-bearing functions get
        // the measured schedule even though the general scheduler conservatively
        // leaves control-flow functions untouched.
        let delayed_save_multiple = legacy_dense.then(|| {
            self.output
                .instructions
                .pop()
                .expect("dense prefix includes stmw")
        });
        let mut delayed_prefix = Some(self.output.instructions.pop().ok_or_else(|| {
            Diagnostic::error("global struct member search is missing its frame prefix")
        })?);
        let mut legacy_link_store =
            if self.behavior.frame_convention == FrameConvention::LinkageFirst {
                Some(self.output.instructions.pop().ok_or_else(|| {
                    Diagnostic::error("legacy global struct member search is missing its LR store")
                })?)
            } else {
                None
            };
        self.emit_address_high(address_high, search.global);
        if let Some(link_store) = legacy_link_store.take() {
            self.output.instructions.push(link_store);
        }
        if self.behavior.frame_convention == FrameConvention::Predecrement {
            self.output
                .instructions
                .push(Instruction::load_immediate(counter, 0));
            self.output
                .instructions
                .push(delayed_prefix.take().expect("frame prefix pending"));
        }
        self.output
            .instructions
            .push(Instruction::load_immediate(GENERAL_SCRATCH, search.bound));
        if self.behavior.frame_convention == FrameConvention::LinkageFirst && !legacy_dense {
            self.output
                .instructions
                .push(Instruction::MoveToCountRegister { s: GENERAL_SCRATCH });
        }
        self.record_relocation(RelocationKind::Addr16Lo, search.global);
        self.output.instructions.push(Instruction::AddImmediate {
            d: base,
            a: address_high,
            immediate: 0,
        });
        if legacy_dense {
            self.output
                .instructions
                .push(Instruction::MoveToCountRegister { s: GENERAL_SCRATCH });
        }
        if self.behavior.frame_convention == FrameConvention::LinkageFirst {
            self.output
                .instructions
                .push(delayed_prefix.take().expect("frame prefix pending"));
            self.output
                .instructions
                .push(Instruction::load_immediate(counter, 0));
        }
        self.output
            .instructions
            .push(Instruction::load_immediate(displacement, 0));
        if let Some(save_multiple) = delayed_save_multiple {
            self.output.instructions.push(save_multiple);
        }
        if self.behavior.frame_convention == FrameConvention::Predecrement {
            self.output
                .instructions
                .push(Instruction::MoveToCountRegister { s: GENERAL_SCRATCH });
            // `ori r0,r0,0` is MWCC's explicit one-cycle issue bubble before the
            // loop-carried address add on mainline compilers.
            self.output.instructions.push(Instruction::OrImmediate {
                a: GENERAL_SCRATCH,
                s: GENERAL_SCRATCH,
                immediate: 0,
            });
        }
        if legacy_dense {
            // In the dense linkage-first frame the counter initialization fills
            // the final save's latency slot immediately before the loop head.
            let counter_init = self
                .output
                .instructions
                .iter()
                .rposition(|instruction| {
                    matches!(instruction, Instruction::AddImmediate { d, a: 0, immediate: 0 } if *d == counter)
                })
                .expect("counter initializer was emitted");
            let instruction = self.output.instructions.remove(counter_init);
            self.output.instructions.push(instruction);
        }
        debug_assert!(delayed_prefix.is_none());
        debug_assert!(legacy_link_store.is_none());

        let loop_head = self.fresh_label();
        let done = self.fresh_label();
        self.bind_label(loop_head);
        self.output.instructions.push(Instruction::Add {
            d: record,
            a: base,
            b: displacement,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: GENERAL_SCRATCH,
            a: record,
            immediate: search.member_offset,
        });
        self.output
            .instructions
            .push(Instruction::CompareLogicalWord {
                a: GENERAL_SCRATCH,
                b: needle,
            });
        if let Some(home) = record_home.filter(|home| *home != record) {
            self.emit_callee_saved_home_copy(home, record);
        }
        self.emit_branch_conditional_to(12, 2, done); // beq
        self.output.instructions.push(Instruction::AddImmediate {
            d: counter,
            a: counter,
            immediate: 1,
        });
        self.output.instructions.push(Instruction::AddImmediate {
            d: displacement,
            a: displacement,
            immediate: search.stride,
        });
        self.emit_branch_conditional_to(16, 0, loop_head); // bdnz
        self.bind_label(done);

        self.locations.insert(
            search.counter.to_string(),
            Location {
                class: ValueClass::General,
                register: counter,
                signed: true,
                width: 32,
                pointee: None,
                stride: None,
            },
        );
        self.locations.insert(
            search.result.to_string(),
            Location {
                class: ValueClass::General,
                register: record_home.unwrap_or(record),
                signed: false,
                width: 32,
                pointee: None,
                stride: Some(u32::try_from(search.stride).expect("positive stride")),
            },
        );
        Ok(true)
    }
}
