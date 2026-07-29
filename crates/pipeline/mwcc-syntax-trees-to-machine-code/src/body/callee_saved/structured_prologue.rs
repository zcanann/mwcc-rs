//! Saved-home store scheduling for general structured bodies.
//!
//! Build 163 establishes every entry-time home before it evaluates pointer
//! initializers when all three lifetime classes are present: entry-initialized
//! locals, an incoming parameter, and a local assigned later in the body. The
//! incoming parameter is copied immediately after its own save; deferred homes
//! follow that copy. Keeping this decision outside the body emitter makes the
//! prologue schedule explicit and independently testable.

#[allow(unused_imports)]
use super::*;

pub(super) fn saved_home_stores_precede_initialization(
    frame_convention: FrameConvention,
    eager_local_count: usize,
    saved_parameter_count: usize,
    deferred_home_count: usize,
) -> bool {
    frame_convention == FrameConvention::LinkageFirst
        && eager_local_count >= 2
        && saved_parameter_count != 0
        && deferred_home_count != 0
}

/// Dense frames without entry-initialized locals may interleave incoming
/// parameter copies with a computed source entry. Once an eager initializer
/// owns the entry, that specialized emitter is bypassed and the ordinary
/// prologue must establish every incoming home itself.
pub(super) fn dense_entry_owns_parameter_copies(
    dense_frame: bool,
    eager_local_count: usize,
) -> bool {
    dense_frame && eager_local_count == 0
}

/// Select MWCC's contiguous GPR save form for structured frames. Eager locals
/// are safe here only when lifetime coloring has merged a later local into an
/// expired parameter home; that is the measured shape where the legacy
/// allocator changes from individually scheduled saves to one dense range.
pub(super) fn uses_dense_saved_register_range(
    with_frame_array: bool,
    eager_local_count: usize,
    saved_home_count: usize,
    global_member_search_entry: bool,
    reuses_parameter_home: bool,
) -> bool {
    with_frame_array
        && saved_home_count <= 18
        && (saved_home_count >= 5 || (global_member_search_entry && saved_home_count >= 4))
        && (eager_local_count == 0 || reuses_parameter_home)
}

impl Generator {
    /// Complete the independent saved-home stores and entry copies before
    /// loading a member-derived saved local. This fills the member load's
    /// latency slots without consuming the incoming receiver before its own
    /// home is established.
    pub(crate) fn schedule_structured_saved_member_entry(&mut self) {
        let Some(start) = self
            .output
            .instructions
            .windows(6)
            .position(is_saved_member_entry)
        else {
            return;
        };
        for from in [start + 2, start + 3, start + 4, start + 5] {
            self.move_instruction_before(from, from - 1);
        }
        let (saved, incoming) = match self.output.instructions[start + 2] {
            Instruction::AddImmediate {
                d,
                a,
                immediate: 0,
            } => (d, a),
            _ => unreachable!("saved member entry was recognized"),
        };
        self.output.instructions[start + 2] =
            Instruction::move_register(saved, incoming);
    }

    /// An incoming float that survives a condition call is copied before the
    /// general saved-parameter pair. The call result itself stays volatile, so
    /// this is the complete mixed-class entry schedule.
    pub(super) fn schedule_transient_condition_float_call_entry(
        &mut self,
        function: &Function,
    ) {
        let has_transient_float_result = function.locals.iter().any(|local| {
            matches!(local.declared_type, Type::Float | Type::Double)
                && super::structured_liveness::transient_condition_call_result_callee(
                    &function.statements,
                    &local.name,
                )
                .is_some()
        });
        if !has_transient_float_result {
            return;
        }
        let Some(call) = self
            .output
            .instructions
            .iter()
            .position(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
        else {
            return;
        };
        let Some(start) = call.checked_sub(3) else {
            return;
        };
        if is_transient_condition_float_entry(&self.output.instructions[start..=call]) {
            self.move_instruction_before(call - 1, start);
        }
    }

    /// Establish a saved entry parameter before deriving another saved local
    /// from it. The generic emitter initially derives through the incoming
    /// register between the two save stores. MWCC completes both physical
    /// saves, copies the entry value, then derives through that durable home.
    pub(super) fn schedule_saved_parameter_derived_initializer(&mut self) {
        let Some(start) = self
            .output
            .instructions
            .windows(4)
            .position(is_saved_parameter_derived_initializer)
        else {
            return;
        };
        self.move_instruction_before(start + 2, start + 1);
        self.move_instruction_before(start + 3, start + 2);
        let (saved, _) = register_copy(&self.output.instructions[start + 2])
            .expect("derived initializer prefix copy was recognized");
        let Instruction::AddImmediate { a, .. } =
            &mut self.output.instructions[start + 3]
        else {
            unreachable!("derived initializer prefix was recognized")
        };
        *a = saved;
    }

    pub(super) fn try_emit_dense_eager_global_array_initializer(
        &mut self,
        initializer: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        let Expression::AddressOf { operand } = initializer else {
            return Ok(false);
        };
        let Expression::Index { base, index } = operand.as_ref() else {
            return Ok(false);
        };
        let (Expression::Variable(global), Expression::Variable(index)) =
            (base.as_ref(), index.as_ref())
        else {
            return Ok(false);
        };
        let Some(&total_size) = self.global_array_sizes.get(global) else {
            return Ok(false);
        };
        if self.behavior.global_addressing == GlobalAddressing::SmallData && total_size <= 8 {
            return Ok(false);
        }
        let Some(&element_type) = self.globals.get(global) else {
            return Ok(false);
        };
        let element_size = match element_type {
            Type::Struct { size, .. } if size != 0 => size,
            _ => u32::from(
                pointee_of_type(element_type)
                    .ok_or_else(|| {
                        Diagnostic::error("dense global-array initializer has no element size")
                    })?
                    .size(),
            ),
        };
        let index = self.lookup_general(index).ok_or_else(|| {
            Diagnostic::error("dense global-array initializer index has no register")
        })?;
        let high = self.fresh_virtual_general_preferring(3);
        let scaled = self.fresh_virtual_general_preferring(5);
        self.emit_address_high(high, global);
        if element_size.is_power_of_two() {
            self.output
                .instructions
                .push(Instruction::ShiftLeftImmediate {
                    a: scaled,
                    s: index,
                    shift: element_size.trailing_zeros() as u8,
                });
        } else {
            let immediate = i16::try_from(element_size).map_err(|_| {
                Diagnostic::error("dense global-array element size is too large to scale")
            })?;
            self.output
                .instructions
                .push(Instruction::MultiplyImmediate {
                    d: scaled,
                    a: index,
                    immediate,
                });
        }
        self.record_relocation(RelocationKind::Addr16Lo, global);
        self.output.instructions.push(Instruction::AddImmediate {
            d: GENERAL_SCRATCH,
            a: high,
            immediate: 0,
        });
        self.output.instructions.push(Instruction::Add {
            d: destination,
            a: GENERAL_SCRATCH,
            b: scaled,
        });
        Ok(true)
    }

    pub(super) fn schedule_dense_eager_initializer(&mut self, start: usize) {
        if !matches!(
            self.output.instructions.get(start),
            Some(Instruction::MultiplyImmediate { .. })
        ) || !matches!(
            self.output.instructions.get(start + 1),
            Some(Instruction::AddImmediateShifted { a: 0, .. })
        ) {
            return;
        }
        self.output.instructions.swap(start, start + 1);
        for relocation in &mut self.output.relocations {
            relocation.instruction_index = match relocation.instruction_index {
                index if index == start => start + 1,
                index if index == start + 1 => start,
                index => index,
            };
        }
    }

    /// The 7400 rounded-pointer frame fills both dependent initializer gaps
    /// with incoming-parameter saves: one between the global high and scale,
    /// then one between the raw pointer adjustment and its alignment mask.
    pub(super) fn schedule_power_pc_7400_rounded_pointer_entry(&mut self) {
        let Some(first_call) = self
            .output
            .instructions
            .iter()
            .position(|instruction| matches!(instruction, Instruction::BranchAndLink { .. }))
        else {
            return;
        };
        let scale = self.output.instructions[..first_call]
            .iter()
            .position(|instruction| matches!(instruction, Instruction::MultiplyImmediate { .. }));
        if let Some(scale) = scale {
            if let Some(parameter_copy) = self.output.instructions[scale + 1..first_call]
                .iter()
                .position(|instruction| {
                    matches!(
                        instruction,
                        Instruction::AddImmediate {
                            a: 4,
                            immediate: 0,
                            ..
                        }
                    )
                })
                .map(|offset| scale + 1 + offset)
            {
                self.move_instruction_before(parameter_copy, scale);
            }
        }

        let Some(mask) = self.output.instructions[..first_call]
            .windows(2)
            .position(|window| {
                matches!(
                    window[0],
                    Instruction::AndContiguousMask { .. }
                        | Instruction::RotateAndMask { .. }
                        | Instruction::ClearLeftImmediate { .. }
                ) && matches!(
                    window[1],
                    Instruction::AddImmediate {
                        immediate: 0,
                        ..
                    }
                )
            })
        else {
            return;
        };
        self.move_instruction_before(mask + 1, mask);
    }

    pub(super) fn try_emit_structured_wide_saved_initializer(
        &mut self,
        initializer: &Expression,
        home: u8,
    ) -> bool {
        let Some(value) = constant_value(initializer) else {
            return false;
        };
        let value = value as i32;
        if (-0x8000..=0x7fff).contains(&value) {
            return false;
        }
        let low = (value as u32 & 0xffff) as i16;
        if low == 0 {
            return false;
        }
        let high_adjusted = ((value - i32::from(low)) >> 16) as i16;
        let scratch = Eabi::general_result().number;
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(scratch, high_adjusted));
        self.output.instructions.push(Instruction::AddImmediate {
            d: home,
            a: scratch,
            immediate: low,
        });
        true
    }

    pub(super) fn emit_structured_saved_home_store(
        &mut self,
        home: u8,
        home_index: usize,
        frame_size: i16,
    ) {
        self.output.instructions.push(Instruction::StoreWord {
            s: home,
            a: 1,
            offset: frame_size - 4 * (home_index as i16 + 1),
        });
    }
}

fn is_transient_condition_float_entry(window: &[Instruction]) -> bool {
    let [
        Instruction::StoreWord {
            s: saved_general,
            a: 1,
            ..
        },
        general_copy,
        Instruction::FloatMove {
            d: saved_float,
            b: incoming_float,
        },
        Instruction::BranchAndLink { .. },
    ] = window
    else {
        return false;
    };
    register_copy(general_copy)
        .is_some_and(|(destination, incoming)| {
            destination == *saved_general
                && destination >= 14
                && (3..=10).contains(&incoming)
        })
        && *saved_float >= 14
        && *incoming_float == Eabi::FIRST_FLOAT_ARGUMENT
}

fn is_saved_member_entry(window: &[Instruction]) -> bool {
    let [
        Instruction::StoreWord {
            s: member_home,
            a: 1,
            ..
        },
        Instruction::LoadWord {
            d: loaded_member,
            a: 3,
            ..
        },
        Instruction::StoreWord {
            s: parameter_home,
            a: 1,
            ..
        },
        Instruction::AddImmediate {
            d: copied_parameter,
            a: incoming_parameter,
            immediate: 0,
        },
        Instruction::StoreWord {
            s: receiver_home,
            a: 1,
            ..
        },
        Instruction::Or {
            a: copied_receiver,
            s: 3,
            b: 3,
        },
    ] = window
    else {
        return false;
    };
    member_home == loaded_member
        && parameter_home == copied_parameter
        && receiver_home == copied_receiver
        && member_home > parameter_home
        && parameter_home > receiver_home
        && (14..=31).contains(member_home)
        && (3..=10).contains(incoming_parameter)
}

fn is_saved_parameter_derived_initializer(window: &[Instruction]) -> bool {
    let [
        Instruction::StoreWord {
            s: derived,
            a: 1,
            ..
        },
        Instruction::AddImmediate {
            d,
            a: incoming,
            ..
        },
        Instruction::StoreWord {
            s: saved,
            a: 1,
            ..
        },
        copy,
    ] = window
    else {
        return false;
    };
    *derived == *d
        && derived != saved
        && register_copy(copy) == Some((*saved, *incoming))
}

fn register_copy(instruction: &Instruction) -> Option<(u8, u8)> {
    match instruction {
        Instruction::Or { a, s, b } if s == b => Some((*a, *s)),
        Instruction::AddImmediate {
            d,
            a,
            immediate: 0,
        } => Some((*d, *a)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_mixed_lifetime_classes_batch_their_saved_home_stores() {
        assert!(saved_home_stores_precede_initialization(
            FrameConvention::LinkageFirst,
            2,
            1,
            1,
        ));
    }

    #[test]
    fn simpler_or_predecrement_prologues_keep_their_existing_schedule() {
        assert!(!saved_home_stores_precede_initialization(
            FrameConvention::LinkageFirst,
            1,
            1,
            1,
        ));
        assert!(!saved_home_stores_precede_initialization(
            FrameConvention::Predecrement,
            2,
            1,
            1,
        ));
    }

    #[test]
    fn only_an_initializer_free_dense_entry_owns_parameter_copies() {
        assert!(dense_entry_owns_parameter_copies(true, 0));
        assert!(!dense_entry_owns_parameter_copies(true, 1));
        assert!(!dense_entry_owns_parameter_copies(false, 0));
    }

    #[test]
    fn expired_parameter_reuse_enables_a_dense_eager_frame() {
        assert!(uses_dense_saved_register_range(true, 4, 12, false, true));
        assert!(!uses_dense_saved_register_range(true, 4, 12, false, false));
    }

    #[test]
    fn recognizes_a_saved_parameter_feeding_a_derived_home() {
        let instructions = [
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 20,
            },
            Instruction::AddImmediate {
                d: 31,
                a: 3,
                immediate: 272,
            },
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 16,
            },
            Instruction::move_register(30, 3),
        ];

        assert!(is_saved_parameter_derived_initializer(&instructions));
    }

    #[test]
    fn recognizes_the_mixed_saved_entry_before_a_float_condition_call() {
        let instructions = [
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 20,
            },
            Instruction::move_register(31, 3),
            Instruction::FloatMove { d: 31, b: 1 },
            Instruction::BranchAndLink {
                target: "produce".into(),
            },
        ];

        assert!(is_transient_condition_float_entry(&instructions));
    }

    #[test]
    fn recognizes_a_member_home_staggered_between_entry_saves() {
        let instructions = [
            Instruction::StoreWord {
                s: 31,
                a: 1,
                offset: 36,
            },
            Instruction::LoadWord {
                d: 31,
                a: 3,
                offset: 44,
            },
            Instruction::StoreWord {
                s: 30,
                a: 1,
                offset: 32,
            },
            Instruction::AddImmediate {
                d: 30,
                a: 5,
                immediate: 0,
            },
            Instruction::StoreWord {
                s: 29,
                a: 1,
                offset: 28,
            },
            Instruction::move_register(29, 3),
        ];

        assert!(is_saved_member_entry(&instructions));
    }
}
