//! Integer<->float conversions.

use crate::generator::*;
use mwcc_core::Compilation;
use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{ArmBody, Expression, Pointee, Statement, Type};

#[derive(Clone, Copy)]
pub(crate) enum IntToFloatSchedule {
    LeafValue,
    CallResult,
}

impl Generator {
    /// Count float-to-integer conversions in a structured body. MWCC assigns
    /// one eight-byte conversion image per syntactic conversion, so frame
    /// owners need this number before they emit their prologue.
    pub(crate) fn count_float_to_integer_conversions(&self, statements: &[Statement]) -> usize {
        fn expression_count(generator: &Generator, expression: &Expression) -> usize {
            match expression {
                Expression::Assign { target, value } => {
                    usize::from(
                        generator.integer_store_target(target)
                            && generator.is_float_value(value),
                    ) + expression_count(generator, target)
                        + expression_count(generator, value)
                }
                Expression::Binary { left, right, .. }
                | Expression::Comma { left, right } => {
                    expression_count(generator, left) + expression_count(generator, right)
                }
                Expression::Cast {
                    target_type,
                    operand,
                } => {
                    usize::from(
                        matches!(
                            target_type,
                            Type::Int
                                | Type::UnsignedInt
                                | Type::Char
                                | Type::UnsignedChar
                                | Type::Short
                                | Type::UnsignedShort
                                | Type::LongLong
                                | Type::UnsignedLongLong
                        ) && (generator.is_float_value(operand)
                            || generator.is_float_operand(operand)
                            || matches!(operand.as_ref(), Expression::Call { name, .. }
                                if matches!(generator.call_return_types.get(name), Some(Type::Float | Type::Double))))
                    ) + expression_count(generator, operand)
                }
                Expression::Unary { operand, .. }
                | Expression::IndexedUpdateValue { value: operand }
                | Expression::Dereference { pointer: operand }
                | Expression::AddressOf { operand }
                | Expression::PostStep {
                    target: operand, ..
                } => expression_count(generator, operand),
                Expression::Conditional {
                    condition,
                    when_true,
                    when_false,
                    ..
                } => {
                    expression_count(generator, condition)
                        + expression_count(generator, when_true)
                        + expression_count(generator, when_false)
                }
                Expression::BitFieldRead {
                    extracted, storage, ..
                }
                | Expression::Index {
                    base: extracted,
                    index: storage,
                } => {
                    expression_count(generator, extracted)
                        + expression_count(generator, storage)
                }
                Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
                    expression_count(generator, base)
                }
                Expression::Call { arguments, .. } => arguments
                    .iter()
                    .map(|argument| expression_count(generator, argument))
                    .sum(),
                Expression::CallThrough { target, arguments } => {
                    expression_count(generator, target)
                        + arguments
                            .iter()
                            .map(|argument| expression_count(generator, argument))
                            .sum::<usize>()
                }
                Expression::VirtualCall {
                    object, arguments, ..
                } => {
                    expression_count(generator, object)
                        + arguments
                            .iter()
                            .map(|argument| expression_count(generator, argument))
                            .sum::<usize>()
                }
                Expression::ConstructedNew {
                    allocation,
                    arguments,
                    ..
                } => {
                    expression_count(generator, allocation)
                        + arguments
                            .iter()
                            .map(|argument| expression_count(generator, argument))
                            .sum::<usize>()
                }
                Expression::AggregateLiteral(elements) => elements
                    .iter()
                    .map(|element| expression_count(generator, element))
                    .sum(),
                Expression::IntegerLiteral(_)
                | Expression::FloatLiteral(_)
                | Expression::StringLiteral(_)
                | Expression::Variable(_)
                | Expression::CompoundLiteral { .. } => 0,
            }
        }

        fn arm_count(generator: &Generator, arm: &ArmBody) -> usize {
            match arm {
                ArmBody::Return(expression) => expression_count(generator, expression),
                ArmBody::Statements(statements) => statement_count(generator, statements),
            }
        }

        fn statement_count(generator: &Generator, statements: &[Statement]) -> usize {
            statements
                .iter()
                .map(|statement| match statement {
                    Statement::Store { target, value } => {
                        usize::from(
                            generator.integer_store_target(target)
                                && generator.is_float_value(value),
                        ) + expression_count(generator, target)
                            + expression_count(generator, value)
                    }
                    Statement::Assign { value, .. } | Statement::Expression(value) => {
                        expression_count(generator, value)
                    }
                    Statement::If {
                        condition,
                        then_body,
                        else_body,
                    } => {
                        expression_count(generator, condition)
                            + statement_count(generator, then_body)
                            + statement_count(generator, else_body)
                    }
                    Statement::Return(value) => value
                        .as_ref()
                        .map_or(0, |value| expression_count(generator, value)),
                    Statement::Switch {
                        scrutinee,
                        arms,
                        default,
                    } => {
                        expression_count(generator, scrutinee)
                            + arms
                                .iter()
                                .map(|arm| arm_count(generator, &arm.body))
                                .sum::<usize>()
                            + default
                                .as_ref()
                                .map_or(0, |arm| arm_count(generator, arm))
                    }
                    Statement::Loop {
                        initializer,
                        condition,
                        step,
                        body,
                        ..
                    } => {
                        initializer
                            .as_ref()
                            .map_or(0, |value| expression_count(generator, value))
                            + condition
                                .as_ref()
                                .map_or(0, |value| expression_count(generator, value))
                            + step
                                .as_ref()
                                .map_or(0, |value| expression_count(generator, value))
                            + statement_count(generator, body)
                    }
                    Statement::Break
                    | Statement::Continue
                    | Statement::Goto(_)
                    | Statement::Label(_)
                    | Statement::InlineAsm(_) => 0,
                })
                .sum()
        }

        statement_count(self, statements)
    }

    fn integer_store_target(&self, target: &Expression) -> bool {
        let pointee = match target {
            Expression::Member { member_type, .. } => {
                crate::expressions::pointee_of_type(*member_type)
            }
            Expression::Dereference { pointer } => self.pointee_of(pointer).ok(),
            Expression::Index { base, .. } => self.pointee_of(base).ok(),
            Expression::Variable(name) => self
                .frame_slots
                .get(name)
                .and_then(|slot| crate::expressions::frame_value_pointee(slot.value_type))
                .or_else(|| {
                    self.globals
                        .get(name)
                        .and_then(|value_type| crate::expressions::pointee_of_type(*value_type))
                }),
            _ => None,
        };
        pointee.is_some_and(|pointee| !matches!(pointee, Pointee::Float | Pointee::Double))
    }

    /// Configure the disjoint eight-byte images used by float-to-integer
    /// conversions inside an already-planned stack frame.
    pub(crate) fn plan_float_to_int_scratch(&mut self, base: i16, count: usize) -> Compilation<()> {
        let bytes = i16::try_from(count.saturating_mul(8))
            .map_err(|_| mwcc_core::Diagnostic::error("float-to-int scratch range is too large"))?;
        self.float_to_int_scratch_next = base;
        self.float_to_int_scratch_end = base
            .checked_add(bytes)
            .ok_or_else(|| mwcc_core::Diagnostic::error("float-to-int scratch range is too large"))?;
        Ok(())
    }

    /// Claim one conversion image. Leaf functions discover these lazily, so
    /// grow the single frame push as additional conversions are encountered.
    /// Callee-saved bodies must pre-plan their range before emitting a prologue.
    fn claim_float_to_int_scratch(&mut self) -> Compilation<i16> {
        if self.float_to_int_scratch_next == 0 {
            if self.non_leaf || self.frame_size != 0 {
                return Err(mwcc_core::Diagnostic::error(
                    "a framed float-to-int conversion needs a pre-planned scratch image",
                ));
            }
            self.float_to_int_scratch_next = 8;
        }
        let offset = self.float_to_int_scratch_next;
        let next = offset
            .checked_add(8)
            .ok_or_else(|| mwcc_core::Diagnostic::error("float-to-int scratch range is too large"))?;
        if self.float_to_int_scratch_end != 0 && next > self.float_to_int_scratch_end {
            return Err(mwcc_core::Diagnostic::error(
                "float-to-int conversion exceeded its planned scratch range",
            ));
        }
        self.float_to_int_scratch_next = next;

        if self.float_to_int_scratch_end == 0 {
            let required = next.saturating_add(15) & !15;
            if self.frame_size == 0 {
                self.frame_size = required;
                self.output
                    .instructions
                    .push(Instruction::StoreWordWithUpdate {
                        s: 1,
                        a: 1,
                        offset: -required,
                    });
            } else if required > self.frame_size {
                let old_size = self.frame_size;
                let Some(Instruction::StoreWordWithUpdate { offset, .. }) = self
                    .output
                    .instructions
                    .iter_mut()
                    .find(|instruction| {
                        matches!(instruction, Instruction::StoreWordWithUpdate {
                            s: 1,
                            a: 1,
                            offset,
                        } if *offset == -old_size)
                    })
                else {
                    return Err(mwcc_core::Diagnostic::error(
                        "a growing float-to-int frame is missing its stack push",
                    ));
                };
                *offset = -required;
                self.frame_size = required;
            }
        }
        Ok(offset)
    }

    /// Convert a floating value to a signed integer through MWCC's `fctiwz`
    /// stack image. Unlike the old leaf-only path, the source may be a memory
    /// load or an arbitrary floating expression.
    fn emit_float_to_signed_integer(
        &mut self,
        operand: &Expression,
        destination: u8,
    ) -> Compilation<()> {
        let leaf_source = if self.is_float_leaf(operand) {
            Some(self.float_register_of_leaf(operand)?)
        } else {
            None
        };
        // A resident leaf can begin its conversion before the independent
        // stack update. A computed value needs the frame established before
        // its loads/arithmetic, exactly as MWCC schedules the two cases.
        let scratch = if let Some(source) = leaf_source {
            self.output
                .instructions
                .push(Instruction::ConvertToIntegerWordZero {
                    d: FLOAT_SCRATCH,
                    b: source,
                });
            self.claim_float_to_int_scratch()?
        } else {
            let scratch = self.claim_float_to_int_scratch()?;
            self.evaluate_float(operand, FLOAT_SCRATCH)?;
            self.output
                .instructions
                .push(Instruction::ConvertToIntegerWordZero {
                    d: FLOAT_SCRATCH,
                    b: FLOAT_SCRATCH,
                });
            scratch
        };
        self.output.has_conversion = true;
        self.output
            .instructions
            .push(Instruction::StoreFloatDouble {
                s: FLOAT_SCRATCH,
                a: 1,
                offset: scratch,
            });
        self.output.instructions.push(Instruction::LoadWord {
            d: destination,
            a: 1,
            offset: scratch + 4,
        });
        Ok(())
    }

    /// The integer width (bits) of a cast's leaf operand, when determinable. Used to
    /// defer a cast-to-float of a narrow (char/short) value: mwcc first widens it to
    /// int (extsb/extsh) and reschedules the magic-constant idiom around that extra
    /// instruction — a sequence not modeled here. `None` (unknown) proceeds as before.
    pub(crate) fn cast_operand_width(&self, operand: &Expression) -> Option<u32> {
        match operand {
            Expression::Variable(name) => self
                .locations
                .get(name)
                .map(|location| location.width as u32)
                .or_else(|| {
                    self.globals
                        .get(name)
                        .map(|global_type| global_type.width() as u32)
                }),
            Expression::Member { member_type, .. } => Some(member_type.width() as u32),
            Expression::Cast { target_type, .. } => Some(target_type.width() as u32),
            Expression::Dereference { pointer } => self
                .pointee_of(pointer)
                .ok()
                .map(|pointee| pointee.element().width() as u32),
            Expression::Index { base, .. } => self
                .pointee_of(base)
                .ok()
                .map(|pointee| pointee.element().width() as u32),
            _ => None,
        }
    }

    /// Emit a cast of an integer operand to a float in `destination` — mwcc's
    /// magic-constant conversion: bias the integer (flip its sign bit), assemble
    /// the double `0x43300000_<biased int>` on the stack, and subtract the bias
    /// `0x4330000000000000`. The bias double lives in `.sdata2`; the `lfd dest,0(0)`
    /// is byte-correct here, but its `R_PPC_EMB_SDA21` relocation and the constant
    /// pool are the next M3 step. Leaf integer operands only.
    pub(crate) fn emit_cast_to_float(
        &mut self,
        operand: &Expression,
        destination: u8,
        double: bool,
    ) -> Compilation<()> {
        // Modern MSL defines `fabsf(float f)` as
        // `(float)__fabs((double)f)`. The widening cast is a register no-op,
        // while the narrowing cast makes mwcc keep the double absolute value
        // in f0 and round it into the result register:
        // `fabs f0,f1; frsp f1,f0`.
        if !double {
            if let Expression::Call { name, arguments } = operand {
                if name == "__fabs" {
                    if let [argument] = arguments.as_slice() {
                        let source_operand = match argument {
                            Expression::Cast {
                                target_type: Type::Double,
                                operand,
                            } if self.is_float_leaf(operand) => Some(operand.as_ref()),
                            expression if self.is_float_leaf(expression) => Some(expression),
                            _ => None,
                        };
                        if let Some(source_operand) = source_operand {
                            let source = self.float_register_of_leaf(source_operand)?;
                            self.output.instructions.push(Instruction::FloatAbsolute {
                                d: FLOAT_SCRATCH,
                                b: source,
                            });
                            self.output.instructions.push(Instruction::RoundToSingle {
                                d: destination,
                                b: FLOAT_SCRATCH,
                            });
                            return Ok(());
                        }
                    }
                }
            }
        }
        if let Expression::FloatLiteral(value) = operand {
            self.load_float_literal(destination, *value, double);
            return Ok(());
        }
        // A cast between floating types needs an instruction only when it
        // NARROWS from double to float. Same-precision casts and float-to-double
        // widening are representation-preserving in an FPR.
        let operand_is_double = self.is_double_value(operand);
        if operand_is_double || self.is_float_operand(operand) {
            let narrows = !double && operand_is_double;
            self.evaluate_float(operand, destination)?;
            if narrows {
                self.output.instructions.push(Instruction::RoundToSingle {
                    d: destination,
                    b: destination,
                });
            }
            return Ok(());
        }
        // A narrow integer (char/short) cast to float is first widened to int with
        // extsb/extsh, and mwcc reschedules the magic-constant idiom around that extra
        // instruction. That sequence is not modeled, so defer rather than emit the
        // int-width idiom unextended (wrong bytes for a negative char/short).
        if self
            .cast_operand_width(operand)
            .is_some_and(|width| width < 32)
            && !self.is_narrow_unsigned_load(operand)?
        {
            return Err(mwcc_core::Diagnostic::error(
                "cast-to-float of a signed narrow (char/short) value is not modeled (roadmap)",
            ));
        }
        // The magic bias goes in a register distinct from the assembled value's f0
        // (FLOAT_SCRATCH): the destination when it isn't f0 (a return into f1), else f1
        // for a value/store into f0 — otherwise the assembled `lfd f0` would overwrite
        // the bias, leaving `fsub f0,f0,f0` = 0.
        const FLOAT_FIRST: u8 = 1; // f1
        let bias_register = if destination != FLOAT_SCRATCH {
            destination
        } else {
            FLOAT_FIRST
        };
        if self.is_narrow_unsigned_load(operand)? {
            return self.emit_loaded_unsigned_int_to_float(
                operand,
                destination,
                double,
                bias_register,
            );
        }
        // Signed narrow loads require an additional extsb/extsh whose placement
        // varies independently from the load and bias schedules.
        self.emit_int_to_float(operand, destination, double, bias_register)
    }

    /// Convert an unsigned byte/halfword memory value to floating point.
    ///
    /// The load itself performs the required zero extension. Its placement
    /// within the magic-bias frame sequence differs in the build-163,
    /// GC/2.0p1, and mainline scheduler families, so keep this loaded-value
    /// schedule separate from the register-leaf and call-result schedules.
    fn emit_loaded_unsigned_int_to_float(
        &mut self,
        operand: &Expression,
        destination: u8,
        double: bool,
        bias_register: u8,
    ) -> Compilation<()> {
        let source = self.fresh_virtual_general();
        if !self.non_leaf && self.frame_size == 0 {
            self.frame_size = 16;
            self.output
                .instructions
                .push(Instruction::StoreWordWithUpdate {
                    s: 1,
                    a: 1,
                    offset: -16,
                });
        } else if self.frame_size < 16 {
            self.frame_size = 16;
        }
        self.output.has_conversion = true;
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(0, 17200));

        if self.behavior.legacy_float_cast_schedule {
            self.evaluate_general(operand, source)?;
            self.load_double_constant(bias_register, 0x4330_0000_0000_0000);
            self.output.instructions.push(Instruction::StoreWord {
                s: source,
                a: 1,
                offset: 12,
            });
            self.output.instructions.push(Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 8,
            });
        } else if self.behavior.float_cast_value_store_first {
            self.evaluate_general(operand, source)?;
            self.output.instructions.push(Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 8,
            });
            self.load_double_constant(bias_register, 0x4330_0000_0000_0000);
            self.output.instructions.push(Instruction::StoreWord {
                s: source,
                a: 1,
                offset: 12,
            });
        } else {
            self.load_double_constant(bias_register, 0x4330_0000_0000_0000);
            self.evaluate_general(operand, source)?;
            self.output.instructions.push(Instruction::StoreWord {
                s: 0,
                a: 1,
                offset: 8,
            });
            self.output.instructions.push(Instruction::StoreWord {
                s: source,
                a: 1,
                offset: 12,
            });
        }

        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: FLOAT_SCRATCH,
            a: 1,
            offset: 8,
        });
        self.output.instructions.push(if double {
            Instruction::FloatSubtractDouble {
                d: destination,
                a: FLOAT_SCRATCH,
                b: bias_register,
            }
        } else {
            Instruction::FloatSubtractSingle {
                d: destination,
                a: FLOAT_SCRATCH,
                b: bias_register,
            }
        });
        Ok(())
    }

    /// The magic-constant int->float idiom into `destination`, with the bias double held in
    /// `bias_register` (caller-chosen so a mixed-arithmetic promotion can place the bias in a
    /// register that avoids the live float operand). Assembles `0x43300000_<biased int>` on the
    /// frame and subtracts the `0x4330..` bias. The operand is an int-width GPR leaf.
    pub(crate) fn emit_int_to_float(
        &mut self,
        operand: &Expression,
        destination: u8,
        double: bool,
        bias_register: u8,
    ) -> Compilation<()> {
        // A signed value flips its sign bit first and subtracts `0x43300000_80000000`; an
        // unsigned value skips the flip and subtracts `0x43300000_00000000`. Bumps the @N counter.
        let signed = self.signedness_of(operand)?;
        let source = self.general_register_of_leaf(operand)?;
        self.frame_size = 16;
        self.output
            .instructions
            .push(Instruction::StoreWordWithUpdate {
                s: 1,
                a: 1,
                offset: -16,
            });
        // A leaf value is already in its register, so the build's usual order applies
        // (GC/2.0p1 stores first, every other build loads the bias first).
        self.emit_int_to_float_body(
            source,
            destination,
            double,
            signed,
            bias_register,
            IntToFloatSchedule::LeafValue,
        );
        Ok(())
    }

    /// The int->float magic body — everything after the `stwu` frame push — for an int
    /// value already in `source` (a GPR). `schedule` distinguishes a leaf value
    /// from a call result so each build can select its measured independent-load
    /// ordering. Assumes a logical 16-byte conversion frame and uses its
    /// `r1+8`/`r1+12` scratch before build-specific frame normalization.
    pub(crate) fn emit_int_to_float_body(
        &mut self,
        source: u8,
        destination: u8,
        double: bool,
        signed: bool,
        bias_register: u8,
        schedule: IntToFloatSchedule,
    ) {
        self.emit_int_to_float_body_at(
            source,
            destination,
            double,
            signed,
            bias_register,
            schedule,
            8,
        );
    }

    pub(crate) fn emit_int_to_float_body_at(
        &mut self,
        source: u8,
        destination: u8,
        double: bool,
        signed: bool,
        bias_register: u8,
        schedule: IntToFloatSchedule,
        scratch: i16,
    ) {
        let bias: u64 = if signed {
            0x4330_0000_8000_0000
        } else {
            0x4330_0000_0000_0000
        };
        self.output.has_conversion = true;
        if self.frame_size < 16 {
            self.frame_size = 16;
        }
        if matches!(schedule, IntToFloatSchedule::CallResult)
            && self.behavior.int_call_result_conversion_style
                == mwcc_versions::IntCallResultConversionStyle::LegacyBiasFirst
        {
            if signed {
                self.output
                    .instructions
                    .push(Instruction::XorImmediateShifted {
                        a: 0,
                        s: source,
                        immediate: 0x8000,
                    });
            }
            self.load_double_constant(bias_register, bias);
            self.output.instructions.push(Instruction::StoreWord {
                s: if signed { 0 } else { source },
                a: 1,
                offset: scratch + 4,
            });
            self.output
                .instructions
                .push(Instruction::load_immediate_shifted(0, 17200));
        } else if self.behavior.legacy_float_cast_schedule {
            if signed {
                self.output
                    .instructions
                    .push(Instruction::XorImmediateShifted {
                        a: 0,
                        s: source,
                        immediate: 0x8000,
                    });
                self.output.instructions.push(Instruction::StoreWord {
                    s: 0,
                    a: 1,
                    offset: scratch + 4,
                });
                self.output
                    .instructions
                    .push(Instruction::load_immediate_shifted(0, 17200));
            } else {
                self.output
                    .instructions
                    .push(Instruction::load_immediate_shifted(0, 17200));
                self.output.instructions.push(Instruction::StoreWord {
                    s: source,
                    a: 1,
                    offset: scratch + 4,
                });
            }
            self.load_double_constant(bias_register, bias);
        } else {
            let value_store_first = match schedule {
                IntToFloatSchedule::LeafValue => self.behavior.float_cast_value_store_first,
                IntToFloatSchedule::CallResult => true,
            };
            if signed {
                self.output
                    .instructions
                    .push(Instruction::XorImmediateShifted {
                        a: source,
                        s: source,
                        immediate: 0x8000,
                    });
            }
            self.output
                .instructions
                .push(Instruction::load_immediate_shifted(0, 17200)); // lis r0, 0x4330
            if value_store_first {
                self.output.instructions.push(Instruction::StoreWord {
                    s: source,
                    a: 1,
                    offset: scratch + 4,
                });
                self.load_double_constant(bias_register, bias);
            } else {
                self.load_double_constant(bias_register, bias);
                self.output.instructions.push(Instruction::StoreWord {
                    s: source,
                    a: 1,
                    offset: scratch + 4,
                });
            }
        }
        self.output.instructions.push(Instruction::StoreWord {
            s: 0,
            a: 1,
            offset: scratch,
        });
        self.output.instructions.push(Instruction::LoadFloatDouble {
            d: FLOAT_SCRATCH,
            a: 1,
            offset: scratch,
        });
        // The bias subtract yields the result at the requested precision: `fsub`
        // for an int->double conversion, `fsubs` for int->float.
        self.output.instructions.push(if double {
            Instruction::FloatSubtractDouble {
                d: destination,
                a: FLOAT_SCRATCH,
                b: bias_register,
            }
        } else {
            Instruction::FloatSubtractSingle {
                d: destination,
                a: FLOAT_SCRATCH,
                b: bias_register,
            }
        });
    }

    /// Emit a cast of a float operand to an integer in `destination`. mwcc
    /// converts with `fctiwz`, then bounces the value through the stack frame.
    /// Leaf float operands only for now; int->float (the constant-pool direction)
    /// is handled separately once .sdata2 lands.
    pub(crate) fn emit_cast_to_integer(
        &mut self,
        target_type: Type,
        operand: &Expression,
        destination: u8,
    ) -> Compilation<()> {
        // `(int)(float)x` / `(int)(double)x` is a ROUND-TRIP conversion, not an identity — a float
        // cannot represent every int exactly, so the value can change. The full int->float->int
        // sequence (constant-pool magic in, fctiwz out) is not modeled for a cast operand, so
        // defer rather than fall through to the integer path, which would cancel both casts and
        // silently drop the conversion (returning x unchanged — a miscompile for large ints).
        if matches!(
            operand,
            Expression::Cast {
                target_type: Type::Float | Type::Double,
                ..
            }
        ) {
            return Err(mwcc_core::Diagnostic::error(
                "an int<-float<-int round-trip cast is not modeled (roadmap)",
            ));
        }
        if self.is_float_value(operand) || self.is_float_operand(operand) {
            // float -> unsigned uses a runtime helper call (the value may exceed
            // INT_MAX, which `fctiwz` cannot represent), not the signed frame bounce.
            if !self.signed_of(target_type) {
                if target_type == Type::UnsignedInt {
                    return self.emit_float_to_unsigned_integer(operand, destination);
                }
                return Err(mwcc_core::Diagnostic::error(
                    "float-to-narrow-unsigned conversion is not modeled (roadmap)",
                ));
            }
            // float -> int: convert, bounce through the frame, then narrow if needed.
            self.emit_float_to_signed_integer(operand, destination)?;
            if target_type.width() < 32 {
                // mwcc does NOT narrow a float -> (char/short) cast with an extend
                // instruction: `return (char)a` leaves the fctiwz int in r3 as-is, and a
                // store truncates via stb/sth. Emitting an extsb/extsh here is a spurious
                // extra instruction; the exact contexts where mwcc does vs does not narrow
                // are not modeled, so defer rather than diff.
                return Err(mwcc_core::Diagnostic::error(
                    "float-to-narrow-int cast narrowing is not modeled (roadmap)",
                ));
            }
            return Ok(());
        }
        // A declared float-returning call is not classified by the ordinary
        // expression type helper, but uses the same load/call + conversion path.
        let is_float_call = matches!(operand, Expression::Call { name, .. }
            if matches!(self.call_return_types.get(name), Some(Type::Float | Type::Double)));
        if is_float_call {
            if !self.signed_of(target_type) {
                if target_type == Type::UnsignedInt {
                    return self.emit_float_to_unsigned_integer(operand, destination);
                }
                return Err(mwcc_core::Diagnostic::error(
                    "float-to-narrow-unsigned conversion is not modeled (roadmap)",
                ));
            }
            self.emit_float_to_signed_integer(operand, destination)?;
            return Ok(());
        }
        // `(unsigned char)<char load>`: the byte load (`lbz`/`lbzx`) already zero-extends to
        // 0..255, which IS the unsigned-char value, so mwcc drops BOTH the signed-promotion
        // extsb and the cast's clrlwi — `(unsigned char)gc` / `(unsigned char)*p` is a bare
        // `lbz`. Emit just the load (raw, no promotion) with no trailing widen. A signed-char
        // global, dereference, member, or array element qualifies regardless of source
        // signedness; a short operand needs the `& 0xff` (its load is wider), and a leaf is
        // handled byte-exactly by the path below.
        let operand_is_char_load = self.is_byte_load(operand)
            || matches!(operand, Expression::Variable(name)
                if !self.locations.contains_key(name.as_str())
                    && matches!(self.globals.get(name.as_str()), Some(Type::Char | Type::UnsignedChar)));
        if target_type == Type::UnsignedChar && operand_is_char_load {
            let saved_truncation_context = self.narrow_truncation_context;
            self.narrow_truncation_context = true;
            let evaluated = self.evaluate_general(operand, destination);
            self.narrow_truncation_context = saved_truncation_context;
            evaluated?;
            return Ok(());
        }
        // int -> int narrowing: place the operand (sub-expression -> scratch),
        // then extend/truncate to the target width into the destination.
        if target_type.width() < 32 {
            // The cast itself narrows (extsb/extsh/clrlwi), so a leaf param/local operand is
            // read RAW — it skips the promotion extsb that the cast's own widen would
            // immediately override: `(unsigned char)a` is `clrlwi r3,r3,24`, not `extsb r0,r3;
            // clrlwi r3,r0,24`, and `(char)char_a` is one `extsb`, not two. A pointer load
            // (`(unsigned char)*p`) keeps its char-load defer (raw-reading it would expose the
            // load's r0-vs-destination register choice — a byte diff). A char GLOBAL is also
            // excluded: mwcc recognizes its `lbz` already zero-extends and drops the cast
            // entirely (`(unsigned char)gc` is a bare `lbz`), a separate fold not modeled here.
            let saved_truncation_context = self.narrow_truncation_context;
            if matches!(operand, Expression::Variable(name) if self.locations.contains_key(name.as_str()))
            {
                self.narrow_truncation_context = true;
            }
            let source = self.place_operand_or_scratch(operand, destination);
            self.narrow_truncation_context = saved_truncation_context;
            let source = source?;
            self.emit_widen(
                destination,
                source,
                target_type.width(),
                self.signed_of(target_type),
            );
        } else {
            self.evaluate_general(operand, destination)?;
        }
        Ok(())
    }
}
