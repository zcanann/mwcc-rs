//! Core drive: parameter assignment, evaluate_body, statement/expression emission, the return tail.

#[allow(unused_imports)]
use super::*;

impl Generator {
    pub(crate) fn assign_parameters(&mut self, function: &Function) -> Compilation<()> {
        self.known_locals = function
            .locals
            .iter()
            .map(|local| local.name.clone())
            .collect();
        let mut next_general = Eabi::FIRST_GENERAL_ARGUMENT;
        let mut next_float = Eabi::FIRST_FLOAT_ARGUMENT;
        for parameter in &function.parameters {
            let class = class_of(parameter.parameter_type)?;
            let register = match class {
                ValueClass::General => {
                    let register = next_general;
                    next_general += 1;
                    register
                }
                ValueClass::Float => {
                    let register = next_float;
                    next_float += 1;
                    register
                }
            };
            let signed = self.signed_of(parameter.parameter_type);
            let pointee = match parameter.parameter_type {
                Type::Pointer(pointee) => Some(pointee),
                _ => None,
            };
            let stride = pointer_stride(parameter.parameter_type);
            self.locations.insert(
                parameter.name.clone(),
                Location {
                    class,
                    register,
                    signed,
                    width: parameter.parameter_type.width(),
                    pointee,
                    stride,
                },
            );
        }
        Ok(())
    }

    /// Emit a function involving a long long (64-bit) value, held in a general-register PAIR
    /// (`r3:r4` = high:low). Only a narrow set of shapes is modeled; the rest defer rather than
    /// fall through to the 32-bit codegen (which emits a single-register result for a 64-bit value).
    pub(crate) fn emit_long_long(&mut self, function: &Function) -> Compilation<()> {
        if std::env::var_os("MWCC_CAPTURE_DEBUG").is_some() {
            eprintln!("long-long function: {function:#?}");
        }
        // Fold trivial long-long LOCALS (each initialized once, no reassignment,
        // and used without duplicating a non-leaf computation) into the return —
        // `long long c = a+b; return c;` is byte-identical to `return a+b;`. This
        // recurses on a locals-free function; the duplication guard keeps a local
        // used twice (`c+c`) deferred for the register keeper we don't model.
        if !function.locals.is_empty()
            && function.statements.is_empty()
            && function.guards.is_empty()
            && function.return_expression.is_some()
            && function.locals.iter().all(|local| {
                local.initializer.is_some() && !local.is_static && local.array_length.is_none()
            })
        {
            let mut values: std::collections::HashMap<String, Expression> =
                std::collections::HashMap::new();
            for local in &function.locals {
                let initializer = local.initializer.as_ref().expect("checked above");
                let folded = crate::value_tracking::substitute(initializer, &values);
                values.insert(local.name.clone(), folded);
            }
            let return_expression = function.return_expression.as_ref().expect("checked above");
            if crate::value_tracking::guard_no_duplication(return_expression, &values).is_ok() {
                let folded_return = crate::value_tracking::substitute(return_expression, &values);
                let stripped = Function {
                    locals: Vec::new(),
                    return_expression: Some(folded_return),
                    ..function.clone()
                };
                return self.emit_long_long(&stripped);
            }
        }
        // A 64-bit GLOBAL WRITE (`void f(long long a){ G = a; }`): store the LOW
        // word (r4) to the symbol+4, then the HIGH word (r3) to the symbol
        // (measured order). A long-long first parameter arrives in r3:r4.
        if function.locals.is_empty()
            && function.guards.is_empty()
            && function.return_type == Type::Void
            && self.behavior.global_addressing == GlobalAddressing::SmallData
        {
            if let [Statement::Store {
                target: Expression::Variable(global),
                value: Expression::Variable(source),
            }] = function.statements.as_slice()
            {
                let stores_ll_param = function.parameters.first().is_some_and(|parameter| {
                    &parameter.name == source
                        && matches!(
                            parameter.parameter_type,
                            Type::LongLong | Type::UnsignedLongLong
                        )
                });
                if stores_ll_param
                    && matches!(
                        self.globals.get(global.as_str()),
                        Some(Type::LongLong | Type::UnsignedLongLong)
                    )
                {
                    let high = Eabi::FIRST_GENERAL_ARGUMENT; // r3 — the param's HIGH word
                    let low = high + 1; //                      r4 — the param's LOW word
                    self.record_relocation_with_addend(RelocationKind::EmbSda21, global, 4);
                    self.output.instructions.push(Instruction::StoreWord {
                        s: low,
                        a: 0,
                        offset: 0,
                    });
                    self.record_relocation(RelocationKind::EmbSda21, global);
                    self.output.instructions.push(Instruction::StoreWord {
                        s: high,
                        a: 0,
                        offset: 0,
                    });
                    self.emit_epilogue_and_return();
                    return Ok(());
                }
            }
            // A 64-bit write THROUGH A POINTER param (`void f(long long* p, long
            // long a){ *p = a; }`): store the LOW word to p+4, then the HIGH word
            // to p (measured). Register layout via a mini-EABI walk over the
            // params (each int/pointer = one GPR; a long-long pair aligns to an
            // odd start), so `(long long* p, long long a)` gives p=r3, a=r5:r6.
            // A struct-member write (`s->v = a`) stores at the member's byte
            // offset; a plain deref (`*p = a`) at offset 0.
            if let [Statement::Store {
                target,
                value: Expression::Variable(source),
            }] = function.statements.as_slice()
            {
                let target_access = match target {
                    Expression::Dereference { pointer } => match pointer.as_ref() {
                        Expression::Variable(name) => Some((name.as_str(), 0i16)),
                        _ => None,
                    },
                    Expression::Member {
                        base,
                        offset,
                        member_type: Type::LongLong | Type::UnsignedLongLong,
                        index_stride: None,
                    } => match base.as_ref() {
                        Expression::Variable(name) => Some((name.as_str(), *offset as i16)),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some((pointer_name, byte_offset)) = target_access {
                    let mut next = Eabi::FIRST_GENERAL_ARGUMENT;
                    let mut pointer_register = None;
                    let mut source_pair = None;
                    for parameter in &function.parameters {
                        match parameter.parameter_type {
                            Type::LongLong | Type::UnsignedLongLong => {
                                if next % 2 == 0 {
                                    next += 1;
                                }
                                if &parameter.name == source {
                                    source_pair = Some((next, next + 1));
                                }
                                next += 2;
                            }
                            Type::Pointer(_) | Type::StructPointer { .. } => {
                                if parameter.name == pointer_name {
                                    pointer_register = Some(next);
                                }
                                next += 1;
                            }
                            Type::Int | Type::UnsignedInt => next += 1,
                            _ => {
                                next = u8::MAX; // an unmodeled param type — bail
                                break;
                            }
                        }
                    }
                    if let (Some(base), Some((high, low))) = (pointer_register, source_pair) {
                        self.output.instructions.push(Instruction::StoreWord {
                            s: low,
                            a: base,
                            offset: byte_offset + 4,
                        });
                        self.output.instructions.push(Instruction::StoreWord {
                            s: high,
                            a: base,
                            offset: byte_offset,
                        });
                        self.emit_epilogue_and_return();
                        return Ok(());
                    }
                }
            }
        }
        if self.try_volatile_long_long_wait(function)? {
            return Ok(());
        }
        if self.try_long_long_serial_fold(function)? {
            return Ok(());
        }
        // Other long-long LOCALS (which need pair spills), guards, and statements are not modeled yet.
        if !function.locals.is_empty()
            || !function.guards.is_empty()
            || !function.statements.is_empty()
        {
            return Err(Diagnostic::error(format!(
                "this long long shape is not modeled yet (roadmap; function '{}')",
                function.name
            )));
        }
        let high = Eabi::general_result().number; // r3 — the result HIGH word
        let low = high + 1; //                       r4 — the result LOW word
        let return_expression = function.return_expression.as_ref().ok_or_else(|| {
            Diagnostic::error("a non-void long long function needs a return value")
        })?;
        let any_long_long_parameter = function.parameters.iter().any(|parameter| {
            matches!(
                parameter.parameter_type,
                Type::LongLong | Type::UnsignedLongLong
            )
        });

        // ===== No long-long PARAMETERS: a long-long RETURN from a constant or a widened 32-bit value.
        if !any_long_long_parameter {
            if !matches!(
                function.return_type,
                Type::LongLong | Type::UnsignedLongLong
            ) {
                return Err(Diagnostic::error(
                    "this long long shape is not modeled yet (roadmap)",
                ));
            }
            // (a) A 64-bit integer CONSTANT — `li low,LOW ; li high,HIGH` (LOW word first, as mwcc
            // emits it). Restricted to words that load with a single `li`.
            if let Some(value) = crate::analysis::constant_value(return_expression) {
                let low_word = value as i32 as i64;
                let high_word = value >> 32;
                if i16::try_from(low_word).is_err() || i16::try_from(high_word).is_err() {
                    return Err(Diagnostic::error(
                        "a wide long long constant needs lis/ori (roadmap)",
                    ));
                }
                self.load_integer_constant(low, low_word);
                self.load_integer_constant(high, high_word);
                self.emit_epilogue_and_return();
                return Ok(());
            }
            // (b) Widen a 32-bit int/unsigned FIRST PARAMETER. It arrives in r3 (= result HIGH), so
            // copy it to LOW, then fill HIGH with its sign (`srawi`) or zero (`li`). A NARROW source
            // (short/char) re-extends differently and defers.
            if let Expression::Variable(name) = return_expression {
                if function
                    .parameters
                    .first()
                    .is_some_and(|parameter| &parameter.name == name)
                {
                    let parameter_type = function.parameters[0].parameter_type;
                    if matches!(parameter_type, Type::Int | Type::UnsignedInt) {
                        self.emit_integer_materialization_copy(low, high);
                        if parameter_type.is_signed() {
                            self.output.instructions.push(
                                Instruction::ShiftRightAlgebraicImmediate {
                                    a: high,
                                    s: high,
                                    shift: 31,
                                },
                            );
                        } else {
                            self.load_integer_constant(high, 0);
                        }
                        self.emit_epilogue_and_return();
                        return Ok(());
                    }
                }
            }
            // (c) A 64-bit GLOBAL read (`return G;`): two SDA21 loads — the HIGH
            // word into r3 at the symbol, the LOW word into r4 at the symbol+4
            // (big-endian). Small-data addressing only; ADDR16 defers.
            if let Expression::Variable(name) = return_expression {
                if matches!(
                    self.globals.get(name.as_str()),
                    Some(Type::LongLong | Type::UnsignedLongLong)
                ) && self.behavior.global_addressing == GlobalAddressing::SmallData
                {
                    self.record_relocation(RelocationKind::EmbSda21, name);
                    self.output.instructions.push(Instruction::LoadWord {
                        d: high,
                        a: 0,
                        offset: 0,
                    });
                    self.record_relocation_with_addend(RelocationKind::EmbSda21, name, 4);
                    self.output.instructions.push(Instruction::LoadWord {
                        d: low,
                        a: 0,
                        offset: 0,
                    });
                    self.emit_epilogue_and_return();
                    return Ok(());
                }
            }
            // (d) DEREFERENCE a long-long POINTER parameter (`return *p;`). p
            // arrives in r3 (= result HIGH), so copy it to r4 first (`mr r4,r3`),
            // load the HIGH word into r3 from p[0] (clobbering p), then the LOW
            // word into r4 from the copy[4] (measured).
            if let Expression::Dereference { pointer } = return_expression {
                if let Expression::Variable(name) = pointer.as_ref() {
                    let is_first_ll_pointer =
                        function.parameters.first().is_some_and(|parameter| {
                            &parameter.name == name
                                && matches!(
                                    parameter.parameter_type,
                                    Type::Pointer(Pointee::LongLong | Pointee::UnsignedLongLong)
                                )
                        });
                    if is_first_ll_pointer {
                        let pointer_register = Eabi::FIRST_GENERAL_ARGUMENT; // r3 — p
                        self.output
                            .instructions
                            .push(Instruction::move_register(low, pointer_register));
                        self.output.instructions.push(Instruction::LoadWord {
                            d: high,
                            a: pointer_register,
                            offset: 0,
                        });
                        self.output.instructions.push(Instruction::LoadWord {
                            d: low,
                            a: low,
                            offset: 4,
                        });
                        self.emit_epilogue_and_return();
                        return Ok(());
                    }
                }
            }
            // (d2) A long-long STRUCT MEMBER read (`return s->v;`) — like the
            // dereference, but at the member's byte offset: copy the base to r4,
            // load HIGH from base+off, LOW from copy+off+4.
            if let Expression::Member {
                base,
                offset,
                member_type: Type::LongLong | Type::UnsignedLongLong,
                index_stride: None,
            } = return_expression
            {
                if let Expression::Variable(name) = base.as_ref() {
                    let is_first_struct_pointer =
                        function.parameters.first().is_some_and(|parameter| {
                            &parameter.name == name
                                && matches!(parameter.parameter_type, Type::StructPointer { .. })
                        });
                    if is_first_struct_pointer {
                        let base_register = Eabi::FIRST_GENERAL_ARGUMENT; // r3 — s
                        let off = *offset as i16;
                        self.output
                            .instructions
                            .push(Instruction::move_register(low, base_register));
                        self.output.instructions.push(Instruction::LoadWord {
                            d: high,
                            a: base_register,
                            offset: off,
                        });
                        self.output.instructions.push(Instruction::LoadWord {
                            d: low,
                            a: low,
                            offset: off + 4,
                        });
                        self.emit_epilogue_and_return();
                        return Ok(());
                    }
                }
            }
            return Err(Diagnostic::error(
                "this long long return shape is not modeled yet (roadmap)",
            ));
        }

        // ===== Long-long PARAMETERS present. Allocate GPR argument registers per the EABI: each
        // int-like param takes one GPR; each long-long param an odd-start GPR pair (aligning up if
        // the next GPR is even), so `f(int x, long long a)` puts x in r3 and a in r5:r6. A float/
        // double/struct param alongside a long long (FPRs or aggregates) and an argument list that
        // overflows r3..r10 both defer.
        let mut next_general = Eabi::FIRST_GENERAL_ARGUMENT;
        let mut param_pair: std::collections::HashMap<&str, (u8, Option<u8>)> =
            std::collections::HashMap::new();
        for parameter in &function.parameters {
            match parameter.parameter_type {
                Type::LongLong | Type::UnsignedLongLong => {
                    if next_general % 2 == 0 {
                        next_general += 1; // a long-long pair starts on an odd register
                    }
                    if next_general + 1 > Eabi::LAST_GENERAL_ARGUMENT {
                        return Err(Diagnostic::error("a long-long argument that overflows to the stack is not modeled yet (roadmap)"));
                    }
                    param_pair.insert(parameter.name.as_str(), (next_general, Some(next_general + 1)));
                    next_general += 2;
                }
                Type::Int | Type::UnsignedInt | Type::Short | Type::UnsignedShort | Type::Char | Type::UnsignedChar
                | Type::Pointer(_) | Type::StructPointer { .. } => {
                    if next_general > Eabi::LAST_GENERAL_ARGUMENT {
                        return Err(Diagnostic::error("an integer argument that overflows to the stack is not modeled yet (roadmap)"));
                    }
                    param_pair.insert(parameter.name.as_str(), (next_general, None));
                    next_general += 1;
                }
                _ => return Err(Diagnostic::error("a float/double/struct parameter alongside a long long is not modeled yet (roadmap)")),
            }
        }

        // (c) TRUNCATE a long-long param to int/unsigned — `(int)a` or implicit — is its LOW word:
        // `mr r3, low(a)`.
        if matches!(function.return_type, Type::Int | Type::UnsignedInt) {
            let truncated = match return_expression {
                Expression::Cast {
                    target_type: Type::Int | Type::UnsignedInt,
                    operand,
                } => operand.as_ref(),
                other => other,
            };
            if let Expression::Variable(name) = truncated {
                if let Some(&(_, Some(low_register))) = param_pair.get(name.as_str()) {
                    self.output
                        .instructions
                        .push(Instruction::move_register(high, low_register));
                    self.emit_epilogue_and_return();
                    return Ok(());
                }
            }
            // EQUALITY of two long-long params (`return a == b;`): XOR the words,
            // OR them, and turn the is-zero test into 0/1 with `cntlzw; srwi 5`
            // (measured: xor r0,al,bl; xor r3,ah,bh; or r3,r0,r3; cntlzw r3,r3;
            // srwi r3,r3,5). r3 = 1 iff all 64 bits match.
            // EQUALITY / INEQUALITY of two long-long params: XOR the words and OR
            // them, then turn the is-zero test into 0/1 — `==` via `cntlzw; srwi 5`
            // (1 iff all bits match), `!=` via `addic r0,r3,-1; subfe r3,r0,r3`
            // (1 iff any bit differs).
            if let Expression::Binary {
                operator: operator @ (BinaryOperator::Equal | BinaryOperator::NotEqual),
                left,
                right,
            } = return_expression
            {
                if let (Expression::Variable(left_name), Expression::Variable(right_name)) =
                    (left.as_ref(), right.as_ref())
                {
                    if let (
                        Some(&(left_high, Some(left_low))),
                        Some(&(right_high, Some(right_low))),
                    ) = (
                        param_pair.get(left_name.as_str()),
                        param_pair.get(right_name.as_str()),
                    ) {
                        self.output.instructions.push(Instruction::Xor {
                            a: GENERAL_SCRATCH,
                            s: left_low,
                            b: right_low,
                        });
                        self.output.instructions.push(Instruction::Xor {
                            a: high,
                            s: left_high,
                            b: right_high,
                        });
                        self.output.instructions.push(Instruction::Or {
                            a: high,
                            s: GENERAL_SCRATCH,
                            b: high,
                        });
                        if matches!(operator, BinaryOperator::Equal) {
                            self.output
                                .instructions
                                .push(Instruction::CountLeadingZeros { a: high, s: high });
                            self.output.instructions.push(
                                Instruction::ShiftRightLogicalImmediate {
                                    a: high,
                                    s: high,
                                    shift: 5,
                                },
                            );
                        } else {
                            self.output
                                .instructions
                                .push(Instruction::AddImmediateCarrying {
                                    d: GENERAL_SCRATCH,
                                    a: high,
                                    immediate: -1,
                                });
                            self.output
                                .instructions
                                .push(Instruction::SubtractFromExtended {
                                    d: high,
                                    a: GENERAL_SCRATCH,
                                    b: high,
                                });
                        }
                        self.emit_epilogue_and_return();
                        return Ok(());
                    }
                }
            }
            // SIGNED ORDERED `a < b` of two long-long params -> 0/1. mwcc biases
            // both high words by 0x8000_0000 (`xoris …,0x8000`) to turn the signed
            // compare into an unsigned subtract, runs the 64-bit subtract, and
            // extracts the final borrow with `subfe r3,r7,r7; neg r3,r3` (measured).
            // Restricted to exactly two long-long params (so r3:r4/r5:r6, r7 free).
            if let Expression::Binary {
                operator: BinaryOperator::Less,
                left,
                right,
            } = return_expression
            {
                if let (Expression::Variable(left_name), Expression::Variable(right_name)) =
                    (left.as_ref(), right.as_ref())
                {
                    let both_signed_ll = function.parameters.len() == 2
                        && function
                            .parameters
                            .iter()
                            .all(|parameter| matches!(parameter.parameter_type, Type::LongLong));
                    if both_signed_ll {
                        if let (
                            Some(&(left_high, Some(left_low))),
                            Some(&(right_high, Some(right_low))),
                        ) = (
                            param_pair.get(left_name.as_str()),
                            param_pair.get(right_name.as_str()),
                        ) {
                            let scratch = right_low + 1; // r7 — the first free volatile past r3:r6
                            self.output
                                .instructions
                                .push(Instruction::XorImmediateShifted {
                                    a: scratch,
                                    s: left_high,
                                    immediate: 0x8000,
                                });
                            self.output
                                .instructions
                                .push(Instruction::XorImmediateShifted {
                                    a: high,
                                    s: right_high,
                                    immediate: 0x8000,
                                });
                            self.output
                                .instructions
                                .push(Instruction::SubtractFromCarrying {
                                    d: GENERAL_SCRATCH,
                                    a: right_low,
                                    b: left_low,
                                });
                            self.output
                                .instructions
                                .push(Instruction::SubtractFromExtended {
                                    d: high,
                                    a: high,
                                    b: scratch,
                                });
                            self.output
                                .instructions
                                .push(Instruction::SubtractFromExtended {
                                    d: high,
                                    a: scratch,
                                    b: scratch,
                                });
                            self.output
                                .instructions
                                .push(Instruction::Negate { d: high, a: high });
                            self.emit_epilogue_and_return();
                            return Ok(());
                        }
                    }
                }
            }
            // EQUALITY / INEQUALITY against the constant ZERO (`a == 0`): mwcc
            // materializes 0 in the next free GPR (r5) and reuses it for BOTH XOR
            // words (0's high == low), then the same is-zero -> 0/1 tail.
            if let Expression::Binary {
                operator: operator @ (BinaryOperator::Equal | BinaryOperator::NotEqual),
                left,
                right,
            } = return_expression
            {
                if let (Expression::Variable(name), Some(0)) =
                    (left.as_ref(), crate::analysis::constant_value(right))
                {
                    if let Some(&(param_high, Some(param_low))) = param_pair.get(name.as_str()) {
                        if function.parameters.len() == 1 {
                            let zero = param_low + 1; // r5 — the next free GPR
                            self.load_integer_constant(zero, 0);
                            self.output.instructions.push(Instruction::Xor {
                                a: GENERAL_SCRATCH,
                                s: param_low,
                                b: zero,
                            });
                            self.output.instructions.push(Instruction::Xor {
                                a: high,
                                s: param_high,
                                b: zero,
                            });
                            self.output.instructions.push(Instruction::Or {
                                a: high,
                                s: GENERAL_SCRATCH,
                                b: high,
                            });
                            if matches!(operator, BinaryOperator::Equal) {
                                self.output
                                    .instructions
                                    .push(Instruction::CountLeadingZeros { a: high, s: high });
                                self.output.instructions.push(
                                    Instruction::ShiftRightLogicalImmediate {
                                        a: high,
                                        s: high,
                                        shift: 5,
                                    },
                                );
                            } else {
                                self.output
                                    .instructions
                                    .push(Instruction::AddImmediateCarrying {
                                        d: GENERAL_SCRATCH,
                                        a: high,
                                        immediate: -1,
                                    });
                                self.output
                                    .instructions
                                    .push(Instruction::SubtractFromExtended {
                                        d: high,
                                        a: GENERAL_SCRATCH,
                                        b: high,
                                    });
                            }
                            self.emit_epilogue_and_return();
                            return Ok(());
                        }
                    }
                }
            }
            return Err(Diagnostic::error(
                "this long long truncation is not modeled yet (roadmap)",
            ));
        }
        if !matches!(
            function.return_type,
            Type::LongLong | Type::UnsignedLongLong
        ) {
            return Err(Diagnostic::error(
                "this long long shape is not modeled yet (roadmap)",
            ));
        }

        // (d) RETURN a long-long param: move its pair into the result pair (a bare `blr` when it is
        // already there — the first parameter). mwcc moves LOW then HIGH (`mr r4,r6 ; mr r3,r5`).
        if let Expression::Variable(name) = return_expression {
            if let Some(&(parameter_high, Some(parameter_low))) = param_pair.get(name.as_str()) {
                if parameter_high != high {
                    self.emit_integer_materialization_copy(low, parameter_low);
                    self.emit_integer_materialization_copy(high, parameter_high);
                }
                self.emit_epilogue_and_return();
                return Ok(());
            }
        }

        // (e) ADD / SUBTRACT two long-long params into the result pair; the LOW word carries into
        // HIGH: `addc r4,r4,r6 ; adde r3,r3,r5` or `subfc r4,r6,r4 ; subfe r3,r5,r3`.
        if let Expression::Binary {
            operator,
            left,
            right,
        } = return_expression
        {
            if let (Expression::Variable(left_name), Expression::Variable(right_name)) =
                (left.as_ref(), right.as_ref())
            {
                if let (Some(&(left_high, Some(left_low))), Some(&(right_high, Some(right_low)))) = (
                    param_pair.get(left_name.as_str()),
                    param_pair.get(right_name.as_str()),
                ) {
                    match operator {
                        BinaryOperator::Add => {
                            self.output.instructions.push(Instruction::AddCarrying {
                                d: low,
                                a: left_low,
                                b: right_low,
                            });
                            self.output.instructions.push(Instruction::AddExtended {
                                d: high,
                                a: left_high,
                                b: right_high,
                            });
                            self.emit_epilogue_and_return();
                            return Ok(());
                        }
                        // subfc rD,rA,rB = rB - rA, so the minuend (left) is `b` and subtrahend (right) is `a`.
                        BinaryOperator::Subtract => {
                            self.output
                                .instructions
                                .push(Instruction::SubtractFromCarrying {
                                    d: low,
                                    a: right_low,
                                    b: left_low,
                                });
                            self.output
                                .instructions
                                .push(Instruction::SubtractFromExtended {
                                    d: high,
                                    a: right_high,
                                    b: left_high,
                                });
                            self.emit_epilogue_and_return();
                            return Ok(());
                        }
                        // Bitwise AND/OR/XOR are word-parallel with NO carry — the LOW
                        // word first, then the HIGH (measured: `and r4,r4,r6; and r3,r3,r5`).
                        BinaryOperator::BitAnd | BinaryOperator::BitOr | BinaryOperator::BitXor => {
                            let (low_op, high_op) = match operator {
                                BinaryOperator::BitAnd => (
                                    Instruction::And {
                                        a: low,
                                        s: left_low,
                                        b: right_low,
                                    },
                                    Instruction::And {
                                        a: high,
                                        s: left_high,
                                        b: right_high,
                                    },
                                ),
                                BinaryOperator::BitOr => (
                                    Instruction::Or {
                                        a: low,
                                        s: left_low,
                                        b: right_low,
                                    },
                                    Instruction::Or {
                                        a: high,
                                        s: left_high,
                                        b: right_high,
                                    },
                                ),
                                _ => (
                                    Instruction::Xor {
                                        a: low,
                                        s: left_low,
                                        b: right_low,
                                    },
                                    Instruction::Xor {
                                        a: high,
                                        s: left_high,
                                        b: right_high,
                                    },
                                ),
                            };
                            self.output.instructions.push(low_op);
                            self.output.instructions.push(high_op);
                            self.emit_epilogue_and_return();
                            return Ok(());
                        }
                        _ => {}
                    }
                }
            }
        }

        // (e1a) ADD a WIDENED signed int param to a long-long param (`a + b`, b an
        // int): sign-extend b into r0 (`srawi r0,b,31`), then `addc low,a_low,b`
        // (carry) and `adde high,a_high,r0` (measured).
        if let Expression::Binary {
            operator: BinaryOperator::Add,
            left,
            right,
        } = return_expression
        {
            if let (Expression::Variable(left_name), Expression::Variable(right_name)) =
                (left.as_ref(), right.as_ref())
            {
                if let (Some(&(left_high, Some(left_low))), Some(&(int_register, None))) = (
                    param_pair.get(left_name.as_str()),
                    param_pair.get(right_name.as_str()),
                ) {
                    let signed = function
                        .parameters
                        .iter()
                        .find(|parameter| &parameter.name == right_name)
                        .is_some_and(|parameter| parameter.parameter_type.is_signed());
                    if signed {
                        self.output
                            .instructions
                            .push(Instruction::ShiftRightAlgebraicImmediate {
                                a: GENERAL_SCRATCH,
                                s: int_register,
                                shift: 31,
                            });
                        self.output.instructions.push(Instruction::AddCarrying {
                            d: low,
                            a: left_low,
                            b: int_register,
                        });
                        self.output.instructions.push(Instruction::AddExtended {
                            d: high,
                            a: left_high,
                            b: GENERAL_SCRATCH,
                        });
                        self.emit_epilogue_and_return();
                        return Ok(());
                    }
                }
            }
        }

        // (e1b) A bitwise op of a long-long param with a WIDENED int param
        // (`a | b`, b an int): sign-extend b into r0 (`srawi r0,b,31`) as its high
        // word, then op the LOW word with b and the HIGH word with r0 (measured).
        if let Expression::Binary {
            operator:
                operator @ (BinaryOperator::BitAnd | BinaryOperator::BitOr | BinaryOperator::BitXor),
            left,
            right,
        } = return_expression
        {
            if let (Expression::Variable(left_name), Expression::Variable(right_name)) =
                (left.as_ref(), right.as_ref())
            {
                if let (Some(&(left_high, Some(left_low))), Some(&(int_register, None))) = (
                    param_pair.get(left_name.as_str()),
                    param_pair.get(right_name.as_str()),
                ) {
                    // SIGNED int only: it sign-extends into a full high word
                    // (`srawi r0,b,31`), then both words op. An UNSIGNED int has a
                    // zero high word, which mwcc folds away (`h|0`/`h^0` emit only
                    // the low op, `h&0` zeroes the high) — deferred until measured.
                    let signed = function
                        .parameters
                        .iter()
                        .find(|parameter| &parameter.name == right_name)
                        .is_some_and(|parameter| parameter.parameter_type.is_signed());
                    if signed {
                        self.output
                            .instructions
                            .push(Instruction::ShiftRightAlgebraicImmediate {
                                a: GENERAL_SCRATCH,
                                s: int_register,
                                shift: 31,
                            });
                        let make = |a: u8, s: u8, b: u8| match operator {
                            BinaryOperator::BitAnd => Instruction::And { a, s, b },
                            BinaryOperator::BitOr => Instruction::Or { a, s, b },
                            _ => Instruction::Xor { a, s, b },
                        };
                        self.output
                            .instructions
                            .push(make(low, left_low, int_register));
                        self.output
                            .instructions
                            .push(make(high, left_high, GENERAL_SCRATCH));
                        self.emit_epilogue_and_return();
                        return Ok(());
                    }
                }
            }
        }

        // (e2) UNARY negate / bitwise-not of a single long-long parameter — the
        // LOW word first, then the HIGH (measured). Negate borrows: `subfic
        // r4,r4,0; subfze r3,r3`. Bitwise-not is word-parallel: `not r4,r4; not
        // r3,r3` (`not` == `nor rD,rS,rS`).
        if let Expression::Unary { operator, operand } = return_expression {
            if let Expression::Variable(name) = operand.as_ref() {
                if let Some(&(param_high, Some(param_low))) = param_pair.get(name.as_str()) {
                    match operator {
                        UnaryOperator::Negate => {
                            self.output
                                .instructions
                                .push(Instruction::SubtractFromImmediate {
                                    d: low,
                                    a: param_low,
                                    immediate: 0,
                                });
                            self.output
                                .instructions
                                .push(Instruction::SubtractFromZeroExtended {
                                    d: high,
                                    a: param_high,
                                });
                            self.emit_epilogue_and_return();
                            return Ok(());
                        }
                        UnaryOperator::BitNot => {
                            self.output.instructions.push(Instruction::Nor {
                                a: low,
                                s: param_low,
                                b: param_low,
                            });
                            self.output.instructions.push(Instruction::Nor {
                                a: high,
                                s: param_high,
                                b: param_high,
                            });
                            self.emit_epilogue_and_return();
                            return Ok(());
                        }
                        UnaryOperator::LogicalNot => {}
                    }
                }
            }
        }

        // (e3) SHIFT a single long-long parameter LEFT or RIGHT by 1. The words
        // shift with a one-bit carry between them via `rlwimi`, and the shifted
        // word passes through r0 (measured):
        //   a<<1: slwi r0,low,1; slwi high,high,1; rlwimi high,low,1,31,31; mr low,r0
        //   a>>1: srawi r0,high,1 (signed) / srwi r0,high,1 (unsigned);
        //         rotlwi low,low,31; rlwimi low,high,31,0,0; mr high,r0
        if let Expression::Binary {
            operator,
            left,
            right,
        } = return_expression
        {
            if let (Expression::Variable(name), Some(1)) =
                (left.as_ref(), crate::analysis::constant_value(right))
            {
                if let Some(&(param_high, Some(param_low))) = param_pair.get(name.as_str()) {
                    let scratch = GENERAL_SCRATCH;
                    match operator {
                        BinaryOperator::ShiftLeft => {
                            self.output
                                .instructions
                                .push(Instruction::ShiftLeftImmediate {
                                    a: scratch,
                                    s: param_low,
                                    shift: 1,
                                });
                            self.output
                                .instructions
                                .push(Instruction::ShiftLeftImmediate {
                                    a: param_high,
                                    s: param_high,
                                    shift: 1,
                                });
                            self.output
                                .instructions
                                .push(Instruction::RotateAndMaskInsert {
                                    a: param_high,
                                    s: param_low,
                                    shift: 1,
                                    begin: 31,
                                    end: 31,
                                });
                            self.output
                                .instructions
                                .push(Instruction::move_register(param_low, scratch));
                            self.emit_epilogue_and_return();
                            return Ok(());
                        }
                        BinaryOperator::ShiftRight => {
                            let signed = matches!(function.return_type, Type::LongLong);
                            if signed {
                                self.output.instructions.push(
                                    Instruction::ShiftRightAlgebraicImmediate {
                                        a: scratch,
                                        s: param_high,
                                        shift: 1,
                                    },
                                );
                            } else {
                                self.output.instructions.push(
                                    Instruction::ShiftRightLogicalImmediate {
                                        a: scratch,
                                        s: param_high,
                                        shift: 1,
                                    },
                                );
                            }
                            self.output.instructions.push(Instruction::RotateAndMask {
                                a: param_low,
                                s: param_low,
                                shift: 31,
                                begin: 0,
                                end: 31,
                            });
                            self.output
                                .instructions
                                .push(Instruction::RotateAndMaskInsert {
                                    a: param_low,
                                    s: param_high,
                                    shift: 31,
                                    begin: 0,
                                    end: 0,
                                });
                            self.output
                                .instructions
                                .push(Instruction::move_register(param_high, scratch));
                            self.emit_epilogue_and_return();
                            return Ok(());
                        }
                        _ => {}
                    }
                }
            }
        }

        // (f0) AND a single long-long parameter with a small NON-NEGATIVE
        // constant (`a & 0xff`, field extraction). mwcc materializes the 64-bit
        // constant — high word 0 in r0, low word in r5 (the next free GPR) — then
        // ANDs both words: `li r0,0; li r5,C; and r4,a_low,r5; and r3,a_high,r0`.
        if function.parameters.len() == 1 {
            if let Expression::Binary {
                operator: BinaryOperator::BitAnd,
                left,
                right,
            } = return_expression
            {
                if let (Expression::Variable(name), Some(constant)) =
                    (left.as_ref(), crate::analysis::constant_value(right))
                {
                    if let Some(&(param_high, Some(param_low))) = param_pair.get(name.as_str()) {
                        if (0..=i64::from(i16::MAX)).contains(&constant) {
                            let constant_low = param_low + 1; // r5 — the next free GPR
                            self.load_integer_constant(GENERAL_SCRATCH, 0);
                            self.load_integer_constant(constant_low, constant);
                            self.output.instructions.push(Instruction::And {
                                a: low,
                                s: param_low,
                                b: constant_low,
                            });
                            self.output.instructions.push(Instruction::And {
                                a: high,
                                s: param_high,
                                b: GENERAL_SCRATCH,
                            });
                            self.emit_epilogue_and_return();
                            return Ok(());
                        }
                    }
                }
            }
        }

        // (f) ADD/SUBTRACT a small CONSTANT to a single long-long parameter. mwcc materializes the
        // 64-bit constant — its LOW word into the next free GPR (r5) and its HIGH word into r0, or
        // just r0 when both words are equal — then `addc`/`adde`. `a - C` lowers as `a + (-C)`.
        // Restricted to a single long-long parameter (so a == result == r3:r4 and r5 is free) and
        // li-sized constant words; a wider constant or a second parameter (dead-register reuse)
        // defers.
        if function.parameters.len() == 1 {
            if let Expression::Binary {
                operator,
                left,
                right,
            } = return_expression
            {
                if matches!(operator, BinaryOperator::Add | BinaryOperator::Subtract) {
                    if let (Expression::Variable(name), Some(constant)) =
                        (left.as_ref(), crate::analysis::constant_value(right))
                    {
                        if param_pair
                            .get(name.as_str())
                            .is_some_and(|&(_, low_word)| low_word.is_some())
                        {
                            let value = if *operator == BinaryOperator::Subtract {
                                constant.wrapping_neg()
                            } else {
                                constant
                            };
                            let low_word = value as i32 as i64;
                            let high_word = value >> 32;
                            if i16::try_from(low_word).is_ok() && i16::try_from(high_word).is_ok() {
                                if low_word == high_word {
                                    self.load_integer_constant(GENERAL_SCRATCH, low_word);
                                    self.output.instructions.push(Instruction::AddCarrying {
                                        d: low,
                                        a: low,
                                        b: GENERAL_SCRATCH,
                                    });
                                    self.output.instructions.push(Instruction::AddExtended {
                                        d: high,
                                        a: high,
                                        b: GENERAL_SCRATCH,
                                    });
                                } else if self.behavior.wide_constant_add_schedule
                                    == WideConstantAddSchedule::SerialScratchWords
                                {
                                    self.load_integer_constant(GENERAL_SCRATCH, low_word);
                                    self.output.instructions.push(Instruction::AddCarrying {
                                        d: low,
                                        a: low,
                                        b: GENERAL_SCRATCH,
                                    });
                                    self.load_integer_constant(GENERAL_SCRATCH, high_word);
                                    self.output.instructions.push(Instruction::AddExtended {
                                        d: high,
                                        a: high,
                                        b: GENERAL_SCRATCH,
                                    });
                                } else {
                                    let low_constant_register = high + 2; // r5 — the next free GPR after r3:r4
                                    self.load_integer_constant(low_constant_register, low_word);
                                    self.load_integer_constant(GENERAL_SCRATCH, high_word);
                                    self.output.instructions.push(Instruction::AddCarrying {
                                        d: low,
                                        a: low,
                                        b: low_constant_register,
                                    });
                                    self.output.instructions.push(Instruction::AddExtended {
                                        d: high,
                                        a: high,
                                        b: GENERAL_SCRATCH,
                                    });
                                }
                                self.emit_epilogue_and_return();
                                return Ok(());
                            }
                        }
                    }
                }
            }
        }

        Err(Diagnostic::error(
            "this long long shape is not modeled yet (roadmap)",
        ))
    }

    pub(crate) fn evaluate_body(&mut self, function: &Function) -> Compilation<()> {
        if std::env::var_os("MWCC_CAPTURE_FUNCTION")
            .is_some_and(|name| name == std::ffi::OsStr::new(&function.name))
        {
            eprintln!("captured function: {function:#?}");
            if let Some(expanded) = self.inline_bodies.expand_calls(function) {
                eprintln!("expanded function: {expanded:#?}");
            }
        }
        // Recursive body transforms can introduce hygienic inline locals after
        // parameter assignment initialized this set. Retain their provenance so
        // local-pointer aliases are not later mistaken for entry parameters.
        self.known_locals
            .extend(function.locals.iter().map(|local| local.name.clone()));
        let calls_skipped_inline = function_calls_any(function, &self.skipped_inline_names)
            || self.inline_bodies.calls_any(function);
        // Drop never-referenced, side-effect-free locals (an unused `int s = 0;`) — mwcc
        // emits nothing for them — then recompile the cleaned function.
        if let Some(cleaned) = remove_dead_locals(function) {
            return self.evaluate_body(&cleaned);
        }
        if let Some(inlined) = inline_immutable_pointer_aliases(function) {
            return self.evaluate_body(&inlined);
        }
        if let Some(scalarized) = scalarize_in_place_aggregate_local(function) {
            return self.evaluate_body(&scalarized);
        }
        if let Some(materialized) = materialize_aggregate_return_temporaries(function) {
            return self.evaluate_body(&materialized);
        }
        // A dead trailing local with a side-effecting (call) initializer becomes a leading statement,
        // so the call is emitted for effect rather than dropped (`int x=g(); return a+b;` → `g();
        // return a+b;`).
        if let Some(hoisted) = hoist_dead_trailing_call_local(function) {
            self.legacy_discarded_call_locals += 1;
            return self.evaluate_body(&hoisted);
        }
        // A body that CONTINUES past an early-return guard parses the guard into the ordered
        // statement list (`if (c) return v; b = b + 1; return b;` → statements [If, Assign]).
        // When the guard reads only names the rest never writes, guard-first and guard-last
        // emission read the same registers — mwcc compiles both orders identically — so hoist
        // it back into `guards` and let the trailing-guard machinery emit it. A tail that
        // still reads the result register's parameter does NOT fold (mwcc branches in the
        // ordered source but folds through a temp in the flat one — order matters), so it
        // stays ordered for try_ordered_early_return_branch.
        if let Some(hoisted) = self.hoist_order_independent_leading_guards(function) {
            return self.evaluate_body(&hoisted);
        }
        // C89 fdlibm locals (`double z; z = x*x;`) normalize into
        // initializers for the float paths, alternating with the guard
        // hoist through this recursion.
        if let Some(cleaned) = normalize_leading_local_assigns(function) {
            return self.evaluate_body(&cleaned);
        }
        // The exact-match whole-function captures (src/captures/) claim FIRST
        // among the templates: they gate on the Debug-AST hash + context
        // fingerprint, so they either reproduce measured bytes exactly or
        // decline with no side effects — a generic template mid-emission
        // defer must not shadow an exact capture (ac __StringWrite).
        if self.try_captures(function)? {
            return Ok(());
        }
        // SDK vector installers retain one fixed destination across a copy,
        // cache flush, ordering barrier, and instruction-cache invalidate.  The
        // destination and the symbol-range operands share one measured schedule.
        if self.try_fixed_address_copy_barrier(function)? {
            return Ok(());
        }
        if self.try_guarded_virtual_forwarder(function)? {
            return Ok(());
        }
        // A callback nested in a large global aggregate, with a by-value aggregate second
        // argument and a ninth stack argument. Claim the complete EABI transaction before
        // broad statement handlers split its address-taken parameter and callback apart.
        if self.try_nested_global_indirect_call(function)? {
            return Ok(());
        }
        if self.try_global_call_store_guard_tail(function)? {
            return Ok(());
        }
        if self.try_indexed_call_store_return(function)? {
            return Ok(());
        }
        if self.try_global_pointer_fallback_getter(function)? {
            return Ok(());
        }
        // Whole-file IPA expansion must claim a verified wrapper before the
        // ordinary sibling-call pass turns its sole call into an external
        // branch. The composed walker owns the caller's complete schedule.
        if self.try_ipa_inlined_pointer_walker(function)? {
            return Ok(());
        }
        // A skipped inline has no callable symbol. Let the retained-body gate
        // below compose it instead of allowing this broad sibling-call path to
        // emit an undefined `bl`/`b` target.
        if !calls_skipped_inline && self.try_tail_call(function)? {
            return Ok(());
        }
        if !calls_skipped_inline && self.try_non_tail_call_forward(function)? {
            return Ok(());
        }
        if !calls_skipped_inline && self.try_conditional_member_select_tail(function)? {
            return Ok(());
        }
        if self.try_legacy_comma_parameter_homes(function)? {
            return Ok(());
        }
        // A leaf `fixed_regs[k] |= C` / `&= C`: one shared materialized base,
        // load/update/store through r0. This is the single-node fixed-RMW schedule.
        if self.try_fixed_address_immediate_rmw(function)? {
            return Ok(());
        }
        if self.try_fixed_address_masked_narrow_return(function)? {
            return Ok(());
        }
        // A seven-field DMA program followed by verified busy-wait and local-RMW
        // helpers is one inlined leaf DAG in mwcc. The interprocedural summaries
        // prove those helper semantics before this call-site schedule can claim.
        if self.try_fixed_rmw_with_inline_tail(function)? {
            return Ok(());
        }
        if self.try_global_queue_pop_transaction(function)? {
            return Ok(());
        }
        if self.try_global_chunked_queue_service(function)? {
            return Ok(());
        }
        // A queue interrupt routine composes two callback-consume arms with
        // verified queue-pop and chunk-service helpers that mwcc inlines.
        if self.try_inlined_queue_interrupt_service(function)? {
            return Ok(());
        }
        if self.try_guarded_queue_initialization(function)? {
            return Ok(());
        }
        if self.try_guarded_pointer_pair_initialization(function)? {
            return Ok(());
        }
        if self.try_conditional_member_callback(function)? {
            return Ok(());
        }
        if self.try_guarded_display_list_packet(function)? {
            return Ok(());
        }
        if self.try_global_aggregate_call_initialization(function)? {
            return Ok(());
        }
        if self.try_global_call_result_guard(function)? {
            return Ok(());
        }
        if self.try_global_aggregate_pop(function)? {
            return Ok(());
        }
        if self.try_global_aggregate_post(function)? {
            return Ok(());
        }
        if self.try_inlined_queue_post_transaction(function)? {
            return Ok(());
        }
        // The allocator-free critical transaction contains both a conditional
        // pointer store and a global-return reload, so it must claim before the
        // conservative cross-statement address-reuse prechecks below.
        if self.try_interrupt_protected_allocator_free(function)? {
            return Ok(());
        }
        // SDK one-time initialization combines an early-return guard, values
        // surviving several calls, scalar-global stores, and a fixed-register
        // RMW. It owns that cross-statement schedule before the generic
        // address-reuse and live-across-call prechecks can reject its pieces.
        if self.try_interrupt_protected_guarded_initialization(function)? {
            return Ok(());
        }
        // A context-switching interrupt handler owns a large address-taken
        // local plus a saved incoming context and a load-once optional global
        // callback. Claim it before generic frame-resident lowering splits the
        // cross-call and conditional-call schedule apart.
        if self.try_context_callback_handler(function)? {
            return Ok(());
        }
        // The TRIG DISPATCHER template claims before the general statement
        // walkers (its leading Assigns would otherwise hit the value-tracking
        // defer).
        if self.try_trig_dispatcher(function)? {
            return Ok(());
        }
        // A byte-class tokenizer owns one stack bitmap and three dependent
        // loops. Lower it as one transaction so their shared registers and
        // frame schedule remain coherent.
        if self.try_byte_class_tokenizer(function)? {
            return Ok(());
        }
        if self.try_ascii_pointer_compare(function)? {
            return Ok(());
        }
        if self.try_ascii_uppercase_loop(function)? {
            return Ok(());
        }
        // The ROTATED LOOP likewise (initialized locals route into value
        // tracking otherwise).
        if self.try_ascii_case_fold_hash_loop(function)? {
            return Ok(());
        }
        if self.try_rotated_loop(function)? {
            return Ok(());
        }
        if self.try_pipelined_copy(function)? {
            return Ok(());
        }
        if self.try_guarded_byte_copy(function)? {
            return Ok(());
        }
        if self.try_ctr_loop(function)? {
            return Ok(());
        }
        if self.try_ctr_pair_loop(function)? {
            return Ok(());
        }
        if self.try_bit_reverse_loop(function)? {
            return Ok(());
        }
        if self.try_xnor_feedback_loop(function)? {
            return Ok(());
        }
        if self.try_norm_loop(function)? {
            return Ok(());
        }
        if self.try_ilogb_diamond(function)? {
            return Ok(());
        }
        if self.try_early_ladder(function)? {
            return Ok(());
        }
        if self.try_indexed_double_return(function)? {
            return Ok(());
        }
        if self.try_punned_pair_ladder(function)? {
            return Ok(());
        }
        if self.try_align_diamond(function)? {
            return Ok(());
        }
        if self.try_writeback_norm(function)? {
            return Ok(());
        }
        // Even a side-effect-free variadic definition receives the EABI
        // parameter-save area. Its self-contained owner runs before the broader
        // variadic gate.
        if self.try_simple_variadic_definition(function)? {
            return Ok(());
        }
        // A non-empty VARIADIC definition only a capture may claim — composing
        // the parameter-save area with arbitrary local/body frames remains open.
        if self.variadic_definition {
            return Err(Diagnostic::error("a variadic function definition is not supported yet (the variadic-register save prologue)"));
        }
        // An INITIALIZED AUTOMATIC local array needs the frame copy-in
        // sequence natively — only a capture claim emits it byte-exactly, so
        // an unclaimed function with one defers here (after the templates).
        if function.locals.iter().any(|local| {
            !local.is_static && local.array_length.is_some() && local.data_bytes.is_some()
        }) {
            return Err(Diagnostic::error(
                "an initialized automatic local array is not supported yet (roadmap)",
            ));
        }
        // An EMPTY body — `T f(args) { }` (MSL's "UNUSED FUNCTION" stubs) —
        // is a single `blr` regardless of return type (measured: pikmin
        // string.c's ten stubs; a non-void return is simply garbage).
        if function.statements.is_empty()
            && function.guards.is_empty()
            && function.return_expression.is_none()
            && function.locals.is_empty()
            && self.frame_slots.is_empty()
        {
            self.output
                .instructions
                .push(Instruction::BranchToLinkRegister);
            return Ok(());
        }
        // A body calling a SKIPPED INLINE defers here — after the exact-match
        // templates (a whole-function capture has the inline flattened into
        // its body); the general paths must never emit a bl to the undefined
        // local (wrong bytes — mwcc inlines it).
        if self.try_inlined_byte_append_loop(function)? {
            return Ok(());
        }
        if self.try_status_indexed_call_loop(function)? {
            return Ok(());
        }
        // `return live * local_call(argument) + C;` either preserves the live
        // parameter across an actual call or lets whole-file IPA substitute
        // the same-TU body. This boundary must precede generic inline-body
        // composition, which otherwise cannot distinguish IPA from noauto.
        if self.try_call_result_product_return(function)? {
            return Ok(());
        }
        if calls_skipped_inline {
            if let Some(expanded) = self.inline_bodies.expand_calls_with_facts(function) {
                self.output.anonymous_label_bump += crate::inline_expansion::ordinal_residue(
                    self.inline_expansion_facts,
                    expanded.statement_body_substitutions,
                    expanded.value_body_substitutions,
                    self.behavior.inline_statement_substitution_label_weight,
                );
                return self.evaluate_body(&expanded.function);
            }
            let mut unresolved: Vec<_> = self
                .skipped_inline_names
                .iter()
                .filter(|name| {
                    let singleton = std::collections::HashSet::from([(*name).clone()]);
                    function_calls_any(function, &singleton)
                })
                .cloned()
                .collect();
            unresolved.sort();
            let suffix = if unresolved.is_empty() {
                String::new()
            } else {
                format!(": {}", unresolved.join(", "))
            };
            return Err(Diagnostic::error(format!(
                "a call to a skipped inline function needs inline expansion (roadmap){suffix}"
            )));
        }
        // A NATIVE caller of a WEAK-MATERIALIZED plain inline defers the same
        // way: mwcc may have re-inlined a trivial body at this call site
        // (measured: ww's mbtowc folds to `blr`), so only a capture claim is safe.
        if !self.weak_materialized_names.is_empty()
            && function_calls_any(function, &self.weak_materialized_names)
        {
            return Err(Diagnostic::error(
                "a call to a weak-materialized inline needs its measured call-site form (roadmap)",
            ));
        }
        if self.try_fpclassify_switch(function)? {
            return Ok(());
        }
        // `F t = gf; t();` — a pure fn-pointer alias feeding only the first call's target
        // folds to the direct `gf();` (identical bytes: the pointer loads at the call).
        if let Some(folded) = inline_first_call_target_alias(function) {
            return self.evaluate_body(&folded);
        }
        // Returning a struct BY VALUE (`struct S f(...) { return s; }`) uses the struct-return
        // ABI — a small struct in r3:r4, a larger one via a hidden pointer argument — which is
        // not modeled. Defer rather than emit a bare `blr` that drops the result (a miscompile:
        // the caller would read the input pointer / stale registers as the returned struct).
        if matches!(function.return_type, Type::Struct { .. }) {
            return Err(Diagnostic::error(format!(
                "returning a struct by value is not supported yet (roadmap; function '{}')",
                function.name
            )));
        }
        // A whole-array float/double constant-init run (`g[0]=1.0f; g[1]=2.0f; …`) uses mwcc's
        // shared-base `stfsu` schedule — claim it before the base-addressed-aggregate pre-check
        // below would defer it as an unscheduled multi-store.
        if self.try_float_array_store_fill(function)? {
            return Ok(());
        }
        // A store to a global AGGREGATE that addresses through a base register (a struct value's
        // non-offset-0 or large field, or any array element) alongside ANOTHER store: mwcc materializes
        // that base (`li rB,g@sda21` / `lis rB,g@ha`) AHEAD of all the stores; our program-order
        // materialization emits it between the stores, so the bytes differ. Defer when such a
        // base-addressed aggregate store is present and the function has two-plus stores of any kind —
        // a lone store, all-offset-0 small-struct fields (direct SDA21), a pointer's members, and scalar
        // globals (no base register) stay byte-exact.
        // THREE-TO-FIVE int-literal member stores into ONE large (ADDR16) struct
        // global, ascending offset order. The register rule (measured N=3/4/5):
        // v0 = r(N+2), the base MIGRATES to r(N+1) (`addi base,r3,@lo`), the
        // remaining values DESCEND from r(N) with r3 recycled after the addi and
        // the LAST value always r0; loads batch, then the stores. Other source
        // orders and 6+ stores keep the shared-base defer.
        if function.statements.len() >= 3
            && function.statements.len() <= 5
            && function.return_type == Type::Void
            && function.return_expression.is_none()
            && function.guards.is_empty()
            && function.locals.is_empty()
            && self.frame_slots.is_empty()
            && !function_makes_call(function)
            && self.behavior.global_addressing == GlobalAddressing::SmallData
        {
            let word_member = |generator: &Self,
                               target: &Expression|
             -> Option<(String, u16, u32, u8)> {
                let Expression::Member {
                    base,
                    offset,
                    member_type,
                    index_stride: None,
                } = target
                else {
                    return None;
                };
                let Expression::Variable(name) = base.as_ref() else {
                    return None;
                };
                if generator.locations.contains_key(name.as_str()) {
                    return None;
                }
                let Some(Type::Struct { size, .. }) = generator.globals.get(name.as_str()).copied()
                else {
                    return None;
                };
                if !matches!(
                    member_type,
                    Type::Int
                        | Type::UnsignedInt
                        | Type::Short
                        | Type::UnsignedShort
                        | Type::Char
                        | Type::UnsignedChar
                ) {
                    return None;
                }
                Some((
                    name.clone(),
                    u16::try_from(*offset).ok()?,
                    size,
                    member_type.width(),
                ))
            };
            let store_by_width = |width: u8, source: u8, base: u8, offset: i16| -> Instruction {
                match width {
                    8 => Instruction::StoreByte {
                        s: source,
                        a: base,
                        offset,
                    },
                    16 => Instruction::StoreHalfword {
                        s: source,
                        a: base,
                        offset,
                    },
                    _ => Instruction::StoreWord {
                        s: source,
                        a: base,
                        offset,
                    },
                }
            };
            let mut plan: Vec<(String, u16, u32, i16, u8)> = Vec::new();
            let mut all_fit = true;
            for statement in &function.statements {
                let Statement::Store {
                    target,
                    value: Expression::IntegerLiteral(value),
                } = statement
                else {
                    all_fit = false;
                    break;
                };
                if !(i16::MIN as i64..=i16::MAX as i64).contains(value) {
                    all_fit = false;
                    break;
                }
                match word_member(self, target) {
                    Some((name, offset, size, width)) => {
                        plan.push((name, offset, size, *value as i16, width))
                    }
                    None => {
                        all_fit = false;
                        break;
                    }
                }
            }
            let count = plan.len();
            // The SMALL (SDA) 3-store form: values r5/r4 lead, the base li r3 lands
            // THIRD (no migration), the last value r0; the offset-0 store folds
            // (measured mixed-width: li r5; li r4; li r3,@gm; li r0; stw r5,@gm(0);
            // sth r4,4(r3); stb r0,6(r3)). The values are VIRTUALS: the physical
            // base li pins r3 through the stores, so the DESCENDING policy (window
            // top r5) derives r5/r4 and spills the last value to scratch r0.
            if all_fit
                && count == 3
                && plan
                    .iter()
                    .all(|(name, _, size, _, _)| name == &plan[0].0 && *size <= 8)
                && plan[0].1 == 0
                && plan.windows(2).all(|pair| pair[0].1 < pair[1].1)
            {
                let name = plan[0].0.clone();
                self.descending_allocation_top = Some(count as u8 + 2);
                let value_virtuals: Vec<u8> =
                    (0..count).map(|_| self.fresh_virtual_general()).collect();
                self.output.instructions.push(Instruction::AddImmediate {
                    d: value_virtuals[0],
                    a: 0,
                    immediate: plan[0].3,
                });
                self.output.instructions.push(Instruction::AddImmediate {
                    d: value_virtuals[1],
                    a: 0,
                    immediate: plan[1].3,
                });
                self.emit_global_array_base(&name, plan[0].2, 3)?;
                self.output.instructions.push(Instruction::AddImmediate {
                    d: value_virtuals[2],
                    a: 0,
                    immediate: plan[2].3,
                });
                self.record_relocation(RelocationKind::EmbSda21, &name);
                self.output
                    .instructions
                    .push(store_by_width(plan[0].4, value_virtuals[0], 0, 0));
                self.output.instructions.push(store_by_width(
                    plan[1].4,
                    value_virtuals[1],
                    3,
                    plan[1].1 as i16,
                ));
                self.output.instructions.push(store_by_width(
                    plan[2].4,
                    value_virtuals[2],
                    3,
                    plan[2].1 as i16,
                ));
                self.emit_epilogue_and_return();
                return Ok(());
            }
            if all_fit
                && count >= 3
                && plan
                    .iter()
                    .all(|(name, _, size, _, _)| name == &plan[0].0 && *size > 8)
                && plan.windows(2).all(|pair| pair[0].1 < pair[1].1)
            {
                let name = plan[0].0.clone();
                // PASS-ARC STEPS 2+3: emitted in NATURAL order (address pair, then
                // the values); the latency-slot fill moves the first `li` into the
                // lis->addi stall slot, and the DESCENDING policy (window top
                // r(N+2)) then derives the measured assignment — v0 at the top,
                // the base next, values descending with r3 recycled, the last
                // value in r0 — schedule and registers both from the pass
                // (fires 851-856; policies landed fires 867-870).
                self.descending_allocation_top = Some(count as u8 + 2);
                let value_virtuals: Vec<u8> =
                    (0..count).map(|_| self.fresh_virtual_general()).collect();
                let base = self.fresh_virtual_general();
                self.record_relocation(RelocationKind::Addr16Ha, &name);
                self.output
                    .instructions
                    .push(Instruction::AddImmediateShifted {
                        d: 3,
                        a: 0,
                        immediate: 0,
                    });
                self.record_relocation(RelocationKind::Addr16Lo, &name);
                self.output.instructions.push(Instruction::AddImmediate {
                    d: base,
                    a: 3,
                    immediate: 0,
                });
                for index in 0..count {
                    self.output.instructions.push(Instruction::AddImmediate {
                        d: value_virtuals[index],
                        a: 0,
                        immediate: plan[index].3,
                    });
                }
                for index in 0..count {
                    self.output.instructions.push(store_by_width(
                        plan[index].4,
                        value_virtuals[index],
                        base,
                        plan[index].1 as i16,
                    ));
                }
                self.emit_epilogue_and_return();
                return Ok(());
            }
        }
        // TWO int-literal member stores into ONE small SDA struct global
        // (`gs.a = 1; gs.b = 2;`): mwcc materializes greedy-early — both values
        // (first -> r4, second -> r0), then the shared base (r3), then the stores:
        // the offset-0 store FOLDS its SDA21, the second goes through the base
        // (measured: li r4,1; li r0,2; li r3,@gs; stw r4,@gs(0); stw r0,4(r3)).
        if let [Statement::Store {
            target: target0,
            value: Expression::IntegerLiteral(value0),
        }, Statement::Store {
            target: target1,
            value: Expression::IntegerLiteral(value1),
        }] = function.statements.as_slice()
        {
            let word_member = |generator: &Self,
                               target: &Expression|
             -> Option<(String, u16, u32)> {
                let Expression::Member {
                    base,
                    offset,
                    member_type,
                    index_stride: None,
                } = target
                else {
                    return None;
                };
                let Expression::Variable(name) = base.as_ref() else {
                    return None;
                };
                if generator.locations.contains_key(name.as_str()) {
                    return None;
                }
                let Some(Type::Struct { size, .. }) = generator.globals.get(name.as_str()).copied()
                else {
                    return None;
                };
                if !matches!(member_type, Type::Int | Type::UnsignedInt) {
                    return None;
                }
                Some((name.clone(), u16::try_from(*offset).ok()?, size))
            };
            if function.return_type == Type::Void
                && function.return_expression.is_none()
                && function.guards.is_empty()
                && function.locals.is_empty()
                && self.frame_slots.is_empty()
                && !function_makes_call(function)
                && self.behavior.global_addressing == GlobalAddressing::SmallData
                && (i16::MIN as i64..=i16::MAX as i64).contains(value0)
                && (i16::MIN as i64..=i16::MAX as i64).contains(value1)
            {
                if let (Some((name0, offset0, size)), Some((name1, offset1, _))) =
                    (word_member(self, target0), word_member(self, target1))
                {
                    // A LARGE (ADDR16) struct's pair: stores keep SOURCE order, the
                    // value `li`s fill the lis/addi latency slots (measured both
                    // orders: lis r3; li r4,v0; addi r3; li r0,v1; stw; stw). An
                    // offset-0 first store instead FOLDS @lo into a `stwu` that also
                    // forms the base (li r4,v0; lis r3; stwu r4,@lo(r3); li r0; stw).
                    if name0 == name1 && offset0 != offset1 && size > 8 {
                        if offset0 == 0 {
                            let first = self.fresh_virtual_general_preferring(4);
                            let second = self.fresh_virtual_general_preferring(0);
                            self.output.instructions.push(Instruction::AddImmediate {
                                d: first,
                                a: 0,
                                immediate: *value0 as i16,
                            });
                            self.record_relocation(RelocationKind::Addr16Ha, &name0);
                            self.output
                                .instructions
                                .push(Instruction::AddImmediateShifted {
                                    d: 3,
                                    a: 0,
                                    immediate: 0,
                                });
                            self.record_relocation(RelocationKind::Addr16Lo, &name0);
                            self.output
                                .instructions
                                .push(Instruction::StoreWordWithUpdate {
                                    s: first,
                                    a: 3,
                                    offset: 0,
                                });
                            self.output.instructions.push(Instruction::AddImmediate {
                                d: second,
                                a: 0,
                                immediate: *value1 as i16,
                            });
                            self.output.instructions.push(Instruction::StoreWord {
                                s: second,
                                a: 3,
                                offset: offset1 as i16,
                            });
                        } else if offset1 == 0 {
                            // The offset-0 store SECOND is unmeasured — defer.
                            return Err(Diagnostic::error("a large-struct store pair ending at offset 0 is not supported yet (roadmap)"));
                        } else {
                            // NATURAL order — the latency-slot fill derives the
                            // measured interleave (lis; li v0; addi; li v1).
                            let first = self.fresh_virtual_general_preferring(4);
                            let second = self.fresh_virtual_general_preferring(0);
                            self.record_relocation(RelocationKind::Addr16Ha, &name0);
                            self.output
                                .instructions
                                .push(Instruction::AddImmediateShifted {
                                    d: 3,
                                    a: 0,
                                    immediate: 0,
                                });
                            self.record_relocation(RelocationKind::Addr16Lo, &name0);
                            self.output.instructions.push(Instruction::AddImmediate {
                                d: 3,
                                a: 3,
                                immediate: 0,
                            });
                            self.output.instructions.push(Instruction::AddImmediate {
                                d: first,
                                a: 0,
                                immediate: *value0 as i16,
                            });
                            self.output.instructions.push(Instruction::AddImmediate {
                                d: second,
                                a: 0,
                                immediate: *value1 as i16,
                            });
                            self.output.instructions.push(Instruction::StoreWord {
                                s: first,
                                a: 3,
                                offset: offset0 as i16,
                            });
                            self.output.instructions.push(Instruction::StoreWord {
                                s: second,
                                a: 3,
                                offset: offset1 as i16,
                            });
                        }
                        self.emit_epilogue_and_return();
                        return Ok(());
                    }
                    // Registers assign by SOURCE order (first store's value -> r4,
                    // second -> r0) while the lis and the stores both run in OFFSET
                    // order (measured both source orders); one member must sit at
                    // offset 0 (its store folds), distinct members only.
                    if name0 == name1
                        && offset0 != offset1
                        && (offset0 == 0 || offset1 == 0)
                        && size <= 8
                    {
                        let first = self.fresh_virtual_general_preferring(4);
                        let second = self.fresh_virtual_general_preferring(0);
                        let mut ordered = [
                            (offset0, *value0 as i16, first),
                            (offset1, *value1 as i16, second),
                        ];
                        ordered.sort_by_key(|&(offset, _, _)| offset);
                        for &(_, value, register) in &ordered {
                            self.output.instructions.push(Instruction::AddImmediate {
                                d: register,
                                a: 0,
                                immediate: value,
                            });
                        }
                        self.emit_global_array_base(&name0, size, 3)?;
                        let [(_, _, first_register), (high_offset, _, second_register)] = ordered;
                        self.record_relocation(RelocationKind::EmbSda21, &name0);
                        self.output.instructions.push(Instruction::StoreWord {
                            s: first_register,
                            a: 0,
                            offset: 0,
                        });
                        self.output.instructions.push(Instruction::StoreWord {
                            s: second_register,
                            a: 3,
                            offset: high_offset as i16,
                        });
                        self.emit_epilogue_and_return();
                        return Ok(());
                    }
                }
            }
        }
        {
            let mut total_store_count = 0u32;
            let mut has_base_addressed_aggregate_store = false;
            for statement in &function.statements {
                let Statement::Store { target, .. } = statement else {
                    continue;
                };
                total_store_count += 1;
                match target {
                    // A struct VALUE global's field: offset 0 of a SMALL struct is a direct SDA21 store
                    // (no base register); a non-zero offset or a LARGE (ADDR16) struct needs the base.
                    Expression::Member { base, offset, .. } => {
                        if let Expression::Variable(name) = base.as_ref() {
                            if let Some(Type::Struct { size, .. }) = self.globals.get(name.as_str())
                            {
                                if *offset != 0 || *size > 8 {
                                    has_base_addressed_aggregate_store = true;
                                }
                            }
                        }
                    }
                    // An array global's element always addresses through a base register (a pointer base
                    // is register-resident already, so it is excluded here).
                    Expression::Index { base, .. } => {
                        if let Expression::Variable(name) = base.as_ref() {
                            if self.global_array_sizes.contains_key(name.as_str()) {
                                has_base_addressed_aggregate_store = true;
                            }
                        }
                    }
                    _ => {}
                }
            }
            if has_base_addressed_aggregate_store && total_store_count >= 2 {
                return Err(Diagnostic::error("a base-addressed global-aggregate store alongside another store needs the shared-base schedule (roadmap)"));
            }
        }
        // `void f(){ if (g REL C) { ext(g); g = C2; } }` — a global reused across the branch
        // (loaded once into r3, tested, then passed to the call). Handled before the pre-check
        // below defers it. See body/callee_saved/conditional.rs.
        if self.try_guarded_global_reuse_call(function)? {
            return Ok(());
        }
        // `if (gi) f(gi);` — a global read in BOTH an if-condition and its then-body. mwcc loads the
        // global ONCE into the argument register, tests it there, and reuses it for the guarded call
        // (`lwz r3,gi; cmpwi r3,0; beq; bl f`); our codegen loads it into the scratch for the test, then
        // RELOADS it for the body — wrong bytes. Defer until that value is reused across the branch. (A
        // parameter condition, or a body that does not read the condition's global, stays byte-exact.)
        for statement in &function.statements {
            if let Statement::If {
                condition,
                then_body,
                ..
            } = statement
            {
                let condition_globals: Vec<&str> = self
                    .globals
                    .keys()
                    .filter(|global| expression_reads_name(condition, global))
                    .map(String::as_str)
                    .collect();
                let body_reads_condition_global =
                    then_body.iter().any(|body_statement| match body_statement {
                        Statement::Expression(expression) => condition_globals
                            .iter()
                            .any(|global| expression_reads_name(expression, global)),
                        Statement::Store { value, .. } => condition_globals
                            .iter()
                            .any(|global| expression_reads_name(value, global)),
                        _ => false,
                    });
                if body_reads_condition_global {
                    return Err(Diagnostic::error(format!(
                        "a global read in both an if-condition and its body needs value reuse across the branch (roadmap; function '{}')",
                        function.name
                    )));
                }
            }
        }
        if self.try_long_long_member_initialize(function)? {
            return Ok(());
        }
        if self.try_control_block_unique_copy(function)? {
            return Ok(());
        }
        if self.try_conditional_member_copy(function)? {
            return Ok(());
        }
        if self.try_guarded_aggregate_update(function)? {
            return Ok(());
        }
        if self.try_inlined_guarded_aggregate_update(function)? {
            return Ok(());
        }
        // Endian scalar wrappers intentionally take the address of a 16/32/64-bit
        // parameter, select its frame image or a reversed stack array, then tail
        // into a byte-buffer call. Claim all widths before the long-long router.
        if self.try_endian_stack_pack(function)? {
            return Ok(());
        }
        if self.try_endian_stack_unpack(function)? {
            return Ok(());
        }
        // A long long (64-bit) value lives in a general-register PAIR — r3:r4 is high:low. Route
        // every long-long-involved function to the dedicated handler so none falls through to the
        // 32-bit codegen (which would emit a single-register result for a 64-bit value — wrong
        // bytes). The handler models a narrow set of shapes and defers the rest.
        if matches!(
            function.return_type,
            Type::LongLong | Type::UnsignedLongLong
        ) || function.parameters.iter().any(|parameter| {
            matches!(
                parameter.parameter_type,
                Type::LongLong | Type::UnsignedLongLong
            )
        }) || function
            .locals
            .iter()
            .any(|local| matches!(local.declared_type, Type::LongLong | Type::UnsignedLongLong))
        {
            return self.emit_long_long(function);
        }
        if self.try_saved_global_exchange(function)? {
            return Ok(());
        }
        // A non-volatile terminal global store/read pair is one value operation.
        // Canonicalize it to the assignment-expression form that owns mwcc's
        // stored-result reuse before the conservative recomputation gate below.
        if let Some(coalesced) = coalesce_terminal_global_store_return(
            function,
            &self.globals,
            &self.volatile_globals,
        ) {
            return self.evaluate_body(&coalesced);
        }
        // `loc = …; return loc` where `loc` is a VARIABLE-INDEXED access (`p[i]`) or a GLOBAL —
        // mwcc reuses the scaled index it already computed (`slwi` once) or the just-stored value,
        // but ours recomputes the index (`slwi` twice) or reloads the global, a byte-different
        // sequence. Defer. (A deref `*p`, a member `s->x`, a const index `p[0]`, and a
        // register param/local are byte-exact and unaffected.)
        if let Some(return_expression) = &function.return_expression {
            for statement in &function.statements {
                if let Statement::Store { target, .. } = statement {
                    if structurally_equal(target, return_expression) {
                        let recomputes_address = matches!(target, Expression::Index { index, .. } if constant_value(index).is_none())
                            || matches!(target, Expression::Variable(name) if self.globals.contains_key(name.as_str()));
                        if recomputes_address {
                            return Err(Diagnostic::error("storing to a variable-indexed or global location then returning it recomputes the address (roadmap)"));
                        }
                    }
                }
            }
        }
        // The guarded scalar sibling has a measured store/return schedule and
        // must claim before the conservative general-family defer below.
        if self.try_guarded_global_constant_store_return(function)? {
            return Ok(());
        }
        // `global = const; return <const or global>` — mwcc's scheduler computes the return value
        // (a `li` for a constant, an SDA `lwz` for a global) BEFORE the global constant store; ours
        // emits the store first. A param return (already in r3) or a deref/index return is
        // byte-exact and unaffected, as is a non-constant or non-global store.
        if let Some(return_expression) = &function.return_expression {
            let return_is_const_or_global = constant_value(return_expression).is_some()
                || matches!(return_expression, Expression::Variable(name) if self.globals.contains_key(name.as_str()));
            if return_is_const_or_global {
                for statement in &function.statements {
                    if let Statement::Store { target, value } = statement {
                        if constant_value(value).is_some()
                            && matches!(target, Expression::Variable(name) if self.globals.contains_key(name.as_str()))
                            && self.global_constant_store_return_plan(function).is_none()
                        {
                            return Err(Diagnostic::error(format!(
                                "a global constant store scheduled around a const/global return is not modeled (roadmap; function '{}')",
                                function.name
                            )));
                        }
                    }
                }
            }
        }
        // mwcc's list scheduler INTERLEAVES an independent POINTER store and the return-value
        // computation to fill latency; our program-order codegen for pointer stores emits the store
        // fully, then the return. Two measured shapes diverge (byte-exact-or-defer — the real fix is
        // routing pointer stores through the Phase-E store scheduler, which treats stores as barriers):
        //   (A) a store followed by a `> 0` / `!= 0` comparison return, whose branchless idiom leads
        //       with `neg r0,x` — mwcc HOISTS that neg above the store (`stw` between neg and its uses).
        //       `< 0` / `== 0` / `<= 0` returns lead with srawi/cntlzw and do NOT hoist (stay byte-exact).
        //   (B) a store whose VALUE needs materialization (a `li` constant, an `lwz` load, or a computed
        //       value) followed by a computed-arithmetic return — mwcc schedules the return compute into
        //       the store's materialize→`stw` latency slot. A bare-register store value (`*p = a`) has
        //       no slot, so `*p=a; return a+1;` stays byte-exact.
        // Condition (A) fires for ANY store (a `neg`-hoist even over a global store DIFFs — the DAG
        // emitter does not model a comparison return). Condition (B) is GATED to POINTER/member targets
        // (`*p`, `p[i]`, `p->x`): a GLOBAL-scalar/aggregate store with a computed-arithmetic return
        // (`g = a+1; return b+2;`) rides the DAG-emitter scheduler, which reproduces mwcc's interleave
        // byte-exact (canaries 542_rand, 1015_dag_emitter) — those must NOT defer here.
        if let Some(return_expression) = &function.return_expression {
            let store_target_is_pointer = |target: &Expression| match target {
                Expression::Dereference { .. } => true,
                Expression::Index { base, .. } | Expression::Member { base, .. } => {
                    matches!(base.as_ref(), Expression::Variable(name)
                        if !self.globals.contains_key(name.as_str()) && !self.global_array_sizes.contains_key(name.as_str()))
                }
                _ => false,
            };
            let store_value_needs_materialization = |value: &Expression| {
                // A direct `stw rN` (a register-resident param/local) needs no leading instruction; a
                // constant, a load, or a computed value all materialize into a register first.
                !matches!(value, Expression::Variable(name) if !self.globals.contains_key(name.as_str()))
            };
            let mut has_store = false;
            let mut has_pointer_store = false;
            let mut has_materialized_pointer_store = false;
            for statement in &function.statements {
                if let Statement::Store { target, value } = statement {
                    has_store = true;
                    if store_target_is_pointer(target) {
                        has_pointer_store = true;
                        has_materialized_pointer_store |= store_value_needs_materialization(value);
                    }
                }
            }
            // (A) a `> 0` / `!= 0` comparison of a register leaf against zero, whose branchless idiom
            // leads with `neg r0,x`. mwcc hoists that neg over ANY store (incl. a global — the DAG
            // emitter does not model these two).
            let neg_leading_comparison = |condition: &Expression| {
                matches!(condition,
                    Expression::Binary { operator: BinaryOperator::Greater | BinaryOperator::NotEqual, left, right }
                        if matches!(left.as_ref(), Expression::Variable(_)) && is_zero_literal(right))
            };
            // (C) the BROADER hoisting comparisons — two-register (`a>b`), vs-nonzero-constant (`a>1`),
            // and logical-not (`!a`, which leads with a HOISTED `cntlzw` unlike the semantically-equal
            // Binary `a==0` that lowers cntlzw into the dest). Their leading xor/subf/subfic/cntlzw
            // hoists ONLY over a POINTER store: a GLOBAL store with these returns rides the DAG emitter
            // byte-exact (`g=a; return a>b;` MATCHes), so gate (C) to pointer targets.
            let comparison_hoists = |condition: &Expression| -> bool {
                match condition {
                    Expression::Unary {
                        operator: UnaryOperator::LogicalNot,
                        operand,
                    } => {
                        matches!(operand.as_ref(), Expression::Variable(_))
                    }
                    Expression::Binary {
                        operator,
                        left,
                        right,
                    } if is_comparison(*operator) => {
                        if !matches!(left.as_ref(), Expression::Variable(_)) {
                            return false;
                        }
                        if is_zero_literal(right) {
                            matches!(operator, BinaryOperator::Greater | BinaryOperator::NotEqual)
                        } else {
                            matches!(right.as_ref(), Expression::Variable(_))
                                || constant_value(right).is_some()
                        }
                    }
                    _ => false,
                }
            };
            // Either predicate may appear as the return itself or as a single guard's branchless
            // const-select (`if(cond) return C1; return C2;`).
            let single_const_guard_condition = if function.guards.len() == 1
                && constant_value(&function.guards[0].value).is_some()
                && constant_value(return_expression).is_some()
            {
                Some(&function.guards[0].condition)
            } else {
                None
            };
            let return_hoists_neg_over_store = neg_leading_comparison(return_expression)
                || single_const_guard_condition.is_some_and(|c| neg_leading_comparison(c));
            let return_comparison_hoists_over_pointer = comparison_hoists(return_expression)
                || single_const_guard_condition.is_some_and(|c| comparison_hoists(c));
            // (B) a computed arithmetic/bitwise/shift or unary return (not a comparison or short-circuit).
            let return_is_computed_arithmetic = match return_expression {
                Expression::Binary { operator, .. } => {
                    !is_comparison(*operator)
                        && !matches!(
                            operator,
                            BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr
                        )
                }
                Expression::Unary { .. } => true,
                _ => false,
            };
            // The measured store+product cluster (the __va_arg diamond arm reduced)
            // is handled before this pre-check defers it. See body/conditional.rs.
            if self.try_store_product_return(function)? {
                return Ok(());
            }
            if self.try_prefixed_store_product_return(function)? {
                return Ok(());
            }
            if self.try_align_store_arm(function)? {
                return Ok(());
            }
            if self.try_const_align_store_return(function)? {
                return Ok(());
            }
            if self.try_va_arg_diamond(function)? {
                return Ok(());
            }
            if (has_store && return_hoists_neg_over_store)
                || (has_pointer_store && return_comparison_hoists_over_pointer)
                || (has_materialized_pointer_store && return_is_computed_arithmetic)
            {
                return Err(Diagnostic::error("a store scheduled around the return-value computation needs the store scheduler (roadmap)"));
            }
        }
        // A function that takes the address of a variable lowers it to a stack
        // slot (frame-resident); this takes over the whole body. Checked first,
        // since an address-taken variable cannot be value-tracked in a register.
        // The frexp family (locals REASSIGNED across a writeback diamond) runs
        // before the inline pass, which cannot fold reassigned locals.
        // The PUNNED-BITS guard + float-tail composition (the k_sin prefix)
        // claims ahead of the frame families: its x-spill frame form and the
        // float DAG tail are one measured unit.
        if self.try_punned_guard_float_return(function)? {
            return Ok(());
        }
        // The DUAL-TAIL float return (`if (c) return A; else return B;`) —
        // two independent float DAGs behind one compare.
        if self.try_dual_tail_float_return(function)? {
            return Ok(());
        }
        // The conditional-local diamond (`if (c) qx = A; else qx = B;` +
        // float tail) — the k_cos qx form, register variant.
        if self.try_conditional_local_float_return(function)? {
            return Ok(());
        }
        if self.try_frexp_family(function)? {
            return Ok(());
        }
        // THE COMPOSER: the full three-arm s_floor ladder.
        if self.try_punned_ladder_writeback(function)? {
            return Ok(());
        }
        // THE MODF LADDER: pointer stores + integral/fraction returns.
        if self.try_punned_modf_ladder(function)? {
            return Ok(());
        }
        // The SHIFT-WRITEBACK family (s_floor arm2's core) parses the
        // un-normalized leading assigns itself — its mutations reassign
        // punned locals, which the initializer normalizer refuses.
        if self.try_punned_shift_writeback(function)? {
            return Ok(());
        }
        // The punned-guard WRITEBACK (the s_floor tail) binds its punned
        // locals to scratch registers — ahead of the inline-away pass that
        // would dissolve them into repeated frame reads.
        if self.try_punned_guard_writeback(function)? {
            return Ok(());
        }
        // The raise family (a fn-pointer local live across calls) likewise.
        if self.try_raise_family(function)? {
            return Ok(());
        }
        // Register locals feeding a frame-resident body (`int hx = *(int*)&x; return
        // f(hx);`) inline away first: the frame path cannot bind them, and once
        // substituted the body is the proven direct form (`return f(*(int*)&x);`).
        if let Some(inlined) = inline_frame_feeding_locals(function) {
            self.frame_feeding_local_pressure =
                Some((inlined.local_count, inlined.repeated_guard_local_count));
            return self.evaluate_body(&inlined.function);
        }
        // A struct-image local passed by address to one call (`GXColor c = {…}; g(&c);`).
        if self.try_struct_image_init_call(function)? {
            return Ok(());
        }
        // Constant member stores into a small struct local, then its address to one call.
        if self.try_struct_member_stores_call(function)? {
            return Ok(());
        }
        // One call with an integer literal and a 4-byte compound-literal argument.
        if self.try_compound_literal_call(function)? {
            return Ok(());
        }
        // `int out=0; read(&out); if(out) use(saved); else use(saved);` combines
        // an address-taken local with a parameter that survives the read call.
        // Its owner composes the frame slot and callee-saved allocation before
        // the ordinary frame path claims the function.
        if self.try_frame_call_then_branch(function)? {
            return Ok(());
        }
        if self.try_guarded_computed_survivor_return(function)?
            || self.try_guarded_computed_survivor_frame(function)?
        {
            return Ok(());
        }
        if self.try_callee_saved_structured_frame_body(function)? {
            return Ok(());
        }
        // A single call-bearing `if (a && b)` is already a complete structured
        // control-flow region. Claim it before broader conditional-call owners
        // can lower the condition as a materialized 0/1 expression and lose
        // MWCC's direct short-circuit branches.
        if is_single_short_circuit_call_if(function)
            && self.try_callee_saved_structured_body(function)?
        {
            return Ok(());
        }
        if self.try_frame_resident(function)? {
            return Ok(());
        }
        // A counting `for (i = 0; i < bound; i++)` loop owns its single local
        // counter, so it is checked before the value-tracking path claims it.
        if self.try_for_counter(function)? {
            return Ok(());
        }
        // A leaf non-counting `while`/`do-while` whose body is pure in-place increments
        // (`while (*p) p++;`) lowers to the rotated form; claimed before value-tracking since the
        // loop-carried increment must emit in place.
        if self.try_emit_increment_while(function)? {
            return Ok(());
        }
        // `while (p) { if (…) return p; p = p->next; }` — a linked-list search: the
        // rotated chase loop with an in-body early return (`bclr`).
        if self.try_list_search_loop(function)? {
            return Ok(());
        }
        // SDK callback queues insert one intrusive node into a priority-sorted doubly linked
        // list. The empty-tail and predecessor repairs form one scheduled control-flow owner.
        if self.try_sorted_intrusive_insert(function)? {
            return Ok(());
        }
        // A callback queue folds the logical-not of every indirect result, then one direct
        // synchronization result, into a saved accumulator before returning its boolean inverse.
        if self.try_queue_callback_fold(function)? {
            return Ok(());
        }
        // MWCC may retain a small static selector out of line while expanding
        // it into an O0 caller. Compose the verified helper summary with the
        // caller's source locals before the ordinary call path treats it as a
        // genuine non-leaf call.
        if self.try_inlined_local_select_access(function)? {
            return Ok(());
        }
        // At -O0, named register locals survive the frontend and occupy descending
        // callee-saved homes even in a leaf. Keep that source-level allocation
        // distinct from the optimized conditional-expression family below.
        if self.try_unoptimized_local_select(function)? {
            return Ok(());
        }
        // A bounded global-array lookup keeps the null fallback in r0 and
        // materializes the selected element address into the same candidate.
        if self.try_range_guarded_array_address(function)? {
            return Ok(());
        }
        // A bounded cursor update is a nested leaf diamond: the error local
        // stays in the first free argument register while the success arm
        // updates a position and conditionally raises a high-water member.
        if self.try_bounded_member_cursor(function)? {
            return Ok(());
        }
        // `T y; if (c) y = A; else y = B; return y;` — both arms assign the returned
        // local, so the whole body is the select `return (c) ? A : B`.
        if self.try_conditional_assign(function)? {
            return Ok(());
        }
        // `T y = INIT; if (c) y = NEW; return y;` (no else) where INIT is a variable ALREADY
        // resident in the result register (the common param-0 case): the clean in-place branch
        // form `<test c>; b<!c>lr; <NEW into result>; blr` (min/max/abs/clamp). NEW may be any
        // evaluable expression (neg/mr/li/add/…), unlike the leaf-only initialized handler below.
        if self.try_conditional_overwrite_inplace(function)? {
            return Ok(());
        }
        // `T y = INIT; if (c) y = NEW; return y;` (no else), constant arms — mwcc lowers the
        // conditional ASSIGN as an early-return branch form (NOT the select/branchless idiom).
        // TWO const-init locals under one narrow guard, returned as their sum — the
        // 2-local init-interleave slice. See body/conditional.rs.
        if self.try_conditional_deref_tail(function)? {
            return Ok(());
        }
        if self.try_narrow_guard_inner_bittest(function)? {
            return Ok(());
        }
        if self.try_narrow_interleave_load_first(function)? {
            return Ok(());
        }
        if self.try_narrow_chained_blocks(function)? {
            return Ok(());
        }
        if self.try_narrow_interleave_two_locals(function)? {
            return Ok(());
        }
        if self.try_narrow_interleave_three_locals(function)? {
            return Ok(());
        }
        if self.try_conditional_assign_initialized(function)? {
            return Ok(());
        }
        // `if (c) { [g = w;] [v = NEW;] } return v;` over a PARAMETER — the in-place
        // diamond with the merge `mr r3,v`, folding to a conditional return when v is r3.
        if self.try_guard_block_mutations(function)? {
            return Ok(());
        }
        if self.try_conditional_reassign_return(function)? {
            return Ok(());
        }
        // Two bitfield updates through a global-state alias, followed by a command/data pair on a
        // fixed port and a narrow dirty-flag clear. This must precede local folding: the alias is
        // what identifies the shared state word and its retained load schedule.
        if self.try_fixed_port_bitfield_update(function)? {
            return Ok(());
        }
        // The indexed sibling computes two copies of a state-array element address and schedules
        // two narrow bit inserts around its loads before writing the updated word to a fixed port.
        if self.try_fixed_port_indexed_bitfield_update(function)? {
            return Ok(());
        }
        // A two-bit enum remap feeds one state-word field and then ORs a dirty bit in another
        // member. Keep the named remap local intact until its register schedule is recognized.
        if self.try_enum_remap_member_update(function)? {
            return Ok(());
        }
        // An MWCC absolute-address aggregate carries scheduler provenance that an explicit cast
        // to the same integer address does not. Preserve that distinction through TU metadata and
        // claim the SDK command/data/dirty-clear schedule before generic constant-address stores.
        if self.try_fixed_address_object_flush(function)? {
            return Ok(());
        }
        // A single state-field insert followed by command/constant and command/state replay pairs
        // on one fixed port. The repeated command materially changes the build-163 schedule.
        if self.try_fixed_port_replay_update(function)? {
            return Ok(());
        }
        // A function's value-tracked locals are folded into its stores and trailing return,
        // then recompiled — `int x = a; gi = x; x = b; gj = x;` becomes `gi = a; gj = b;`,
        // and `int x = a; gi = x; return x;` becomes `gi = a; return a;`. The store paths
        // (or the un-schedulable-store deferral) own the cleaned body. Checked before the
        // value-tracking path, which cannot fold a void function's store-feeding locals.
        if let Some(inlined) = inline_store_bearing_locals(function) {
            return self.evaluate_body(&inlined);
        }
        // `int x = foo(...); gi = x;` / `int x = foo(...); return x;` — a single-use call
        // result stored directly or returned. The result lives in r3 and is not live across
        // another call, so both are byte-exact; inline the local. A second call or use
        // defers to the callee-saved allocator.
        if let Some(inlined) = inline_single_call_result(function) {
            return self.evaluate_body(&inlined);
        }
        // Value-tracked locals (reassignment, multiple locals) are inlined into the
        // return expression and compiled there; this takes over the whole body when
        // it applies, leaving the straight-line paths below byte-identical.
        // A single ordered early-return guard over a value-tracked continuation, where the
        // constant fold does not apply — the real forward-branch form.
        if self.try_guarded_member_initialization(function)? {
            return Ok(());
        }
        if self.try_ordered_early_return_branch(function)? {
            return Ok(());
        }
        // The FLOAT DAG arm claims double multiply-add trees with named
        // double locals BEFORE value tracking and the int-oriented folds:
        // folding a single-use float local (v = z*x) duplicates the shared z
        // subterm, while mwcc keeps locals as window-top-tier shared
        // registers.
        if self.try_float_dag_return(function)? {
            self.output
                .instructions
                .push(Instruction::BranchToLinkRegister);
            return Ok(());
        }
        if self.try_float_param_reassign(function)? {
            return Ok(());
        }
        if self.try_live_across_branches(function)? {
            return Ok(());
        }
        // `int t = <single-op>; *p = t; return t;` — a computed local kept in r3 and
        // stored from there, rather than inlined (recomputed) by value_tracking. See
        // body/store_fill.rs.
        if self.try_computed_local_stored_returned(function)? {
            return Ok(());
        }
        // An SDK flush primitive writes a header to a fixed port, emits a modulo-scheduled
        // eight-word zero-fill loop, then marks the owning global structure flushed.
        if self.try_fixed_port_zero_fill(function)? {
            return Ok(());
        }
        if self.try_constructor_constant_store_fill(function)? {
            return Ok(());
        }
        if self.try_value_tracking(function)? {
            return Ok(());
        }
        // Fold single-assignment, return-only locals (no call in their initializers)
        // into the return, then recompile — `int z = x + 1; g(); return z;` becomes the
        // equivalent `g(); return x + 1;`, which the parameter-preservation path emits.
        if let Some(inlined) = inline_return_only_locals(function) {
            return self.evaluate_body(&inlined);
        }
        // A value-tracked local feeding a single switch's scrutinee/arms inlines into the switch and
        // recompiles, so `int m = n + 1; switch(m)` lowers like the direct `switch(n + 1)`.
        if let Some(inlined) = inline_switch_scrutinee_locals(function) {
            return self.evaluate_body(&inlined);
        }
        // A leaf void body that is purely constant stores of one repeated value
        // (struct/array zeroing) materializes the value once and reuses it.
        if self.try_constant_store_fill(function)? {
            return Ok(());
        }
        if self.try_member_parameter_two_constant_fill(function)? {
            return Ok(());
        }
        if self.try_member_copy_then_call(function)? {
            return Ok(());
        }
        if self.try_narrow_member_initialization(function)? {
            return Ok(());
        }
        // The float sibling: a leaf void body of float-literal stores to `float` globals
        // (`gf=1.0f; gg=2.0f;`) pre-loads each into a distinct FPR, then stores.
        if self.try_float_constant_store_fill(function)? {
            return Ok(());
        }
        // The member sibling: float-literal stores to consecutive members through one pointer base
        // (`p->x=1.0f; p->y=2.0f; p->z=3.0f;`) run mwcc's two-FPR software pipeline, staying two
        // loads ahead of the stores rather than the naive load/store/load/store.
        if self.try_float_member_store_fill(function)? {
            return Ok(());
        }
        // The mixed sibling: exactly one integer-member and one float-member store through one base
        // (`p->i=0; p->f=1.0f;`) — mwcc materializes both values (r0 + f0) then both stores.
        if self.try_mixed_member_store_fill(function)? {
            return Ok(());
        }
        // LEAK GUARD: a leaf void body of 2+ literal member stores through ONE base
        // that mixes integer and float members and was NOT claimed by the fills above
        // (3+ mixed statements). mwcc pipelines the materializations across BOTH
        // register files (loads greedy-early, each >= 2 slots before its store, an
        // extra GPR when reuse would stall — measured `p->i=1; p->f=2.0f; p->j=3;`
        // gives li/lfs/stw/li/stfs/stw); the sequential path would emit wrong bytes,
        // so DEFER until that store scheduler exists.
        if function.return_type == Type::Void
            && function.guards.is_empty()
            && function.locals.is_empty()
            && !function_makes_call(function)
            && function.statements.len() >= 3
        {
            let mut base: Option<&str> = None;
            let mut has_float = false;
            let mut has_integer = false;
            let mut all_literal_member_stores = true;
            for statement in &function.statements {
                let Statement::Store {
                    target:
                        Expression::Member {
                            base: member_base,
                            member_type,
                            index_stride: None,
                            ..
                        },
                    value: Expression::FloatLiteral(_) | Expression::IntegerLiteral(_),
                } = statement
                else {
                    all_literal_member_stores = false;
                    break;
                };
                let Expression::Variable(name) = member_base.as_ref() else {
                    all_literal_member_stores = false;
                    break;
                };
                match base {
                    None => base = Some(name.as_str()),
                    Some(existing) if existing == name.as_str() => {}
                    Some(_) => {
                        all_literal_member_stores = false;
                        break;
                    }
                }
                match member_type {
                    Type::Float | Type::Double => has_float = true,
                    _ => has_integer = true,
                }
            }
            if all_literal_member_stores && has_float && has_integer {
                return Err(Diagnostic::error(
                    "a mixed integer/float member-store run needs the store scheduler (roadmap)",
                ));
            }
        }
        // A whole-body `if (c) { <constant run> } else { <constant run> }`: branch over the then-arm
        // to the else, each arm the batched constant store run then its own `blr`.
        if self.try_constant_store_if_else(function)? {
            return Ok(());
        }
        // Two computed-value stores to distinct SDA globals: mwcc overlaps the two value
        // computations (both into registers, then both stores), which the sequential path
        // does not. The allocator places the first value off the scratch (live across the
        // second), the second into r0.
        if self.try_computed_store_fill(function)? {
            return Ok(());
        }
        // The same overlap with one computed value and one register-leaf value (`gi=a+1;
        // gj=b;`): the leaf is stored first (ready), the computed second.
        if self.try_mixed_store_fill(function)? {
            return Ok(());
        }
        // Three+ stores of register leaves with a single constant interspersed (`gi=a;
        // gj=b; gk=5;`): the constant's `li` is hoisted and the stores keep source order
        // (a leading constant swaps off the latency slot).
        if self.try_leaf_constant_fill(function)? {
            return Ok(());
        }
        if self.try_legacy_delayed_result_store_run(function)? {
            return Ok(());
        }
        // Leaf multi-store bodies of COMPUTED int values through the measured
        // models — the DAG emitter (linearize + assign_registers). Runs after
        // the proven store-fill arms, catching what they defer.
        if self.try_dag_store_fill(function)? {
            return Ok(());
        }
        // Multiple stores where a value loads a float/double global reschedule the loads
        // (mwcc loads the global once and reuses it across the stores); not modeled, so
        // DEFER rather than emit a redundant load per store. A single such store (`gf =
        // gg;`) needs no scheduling and stays byte-exact.
        if function.guards.is_empty()
            && function.locals.is_empty()
            && !function_makes_call(function)
            && function.statements.len() >= 2
        {
            let loads_float_global = |generator: &Self, value: &Expression| {
                matches!(value, Expression::Variable(name)
                    if !generator.locations.contains_key(name.as_str())
                        && matches!(generator.globals.get(name.as_str()), Some(Type::Float | Type::Double)))
            };
            let all_stores = function
                .statements
                .iter()
                .all(|statement| matches!(statement, Statement::Store { .. }));
            let any_float_global = function
                .statements
                .iter()
                .any(|statement| matches!(statement, Statement::Store { value, .. } if loads_float_global(self, value)));
            if all_stores && any_float_global {
                return Err(Diagnostic::error(
                    "multiple stores loading a float global need the load scheduler (roadmap)",
                ));
            }
        }
        // Un-schedulable multi-store: a body whose statements are 2+ stores to SDA integer
        // globals that the fills above did not absorb (a trailing return, if any, is
        // separate). mwcc latency-schedules these (load/computation hoisting, constant-`li`
        // slot fill); the normal sequential emission would not reproduce that, so DEFER
        // rather than ship wrong bytes. Only an all-distinct-leaf run (no computation to
        // schedule, no dead store) stays byte-exact on the normal path, so let that through.
        if function.guards.is_empty()
            && function.locals.is_empty()
            && !function_makes_call(function)
            && function.statements.len() >= 2
            && self.behavior.global_addressing == GlobalAddressing::SmallData
        {
            let mut targets = Vec::new();
            let mut all_leaves = true;
            let mut all_sda_integer_stores = true;
            for statement in &function.statements {
                let Statement::Store {
                    target: Expression::Variable(name),
                    value,
                } = statement
                else {
                    all_sda_integer_stores = false;
                    break;
                };
                match self.globals.get(name.as_str()) {
                    Some(global_type) if !matches!(global_type, Type::Float | Type::Double) => {
                        targets.push(name.as_str())
                    }
                    _ => {
                        all_sda_integer_stores = false;
                        break;
                    }
                }
                if !matches!(value, Expression::Variable(leaf) if !self.globals.contains_key(leaf.as_str()))
                {
                    all_leaves = false;
                }
            }
            if all_sda_integer_stores {
                let distinct = {
                    let mut sorted = targets.clone();
                    sorted.sort_unstable();
                    sorted.dedup();
                    sorted.len() == targets.len()
                };
                if !all_leaves || !distinct {
                    return Err(Diagnostic::error(
                        "a run of stores that mwcc latency-schedules needs the scheduler (roadmap)",
                    ));
                }
            }
        }
        // A run of POINTER-base stores whose LAST value is COMPUTED with 2+ preceding
        // stores: mwcc's latency scheduler HOISTS that computation up past an intervening
        // store to fill the pipeline slot (`stw; add; stw; stw` for `p[0]=a; p[1]=b;
        // p[2]=a+b;`), which sequential emission would not reproduce — defer. A last leaf
        // value, a computation adjacent to its store (only 2 stores), or an earlier/middle
        // computed value (last store a ready leaf) needs no hoist and stays byte-exact.
        if function.guards.is_empty()
            && function.locals.is_empty()
            && !function_makes_call(function)
            && function.return_type == Type::Void
            && function.statements.len() >= 3
            && function.statements.iter().all(|statement| {
                matches!(
                    statement,
                    Statement::Store {
                        target: Expression::Index { .. } | Expression::Dereference { .. },
                        ..
                    }
                )
            })
        {
            if let Some(Statement::Store { value, .. }) = function.statements.last() {
                let last_is_computed =
                    constant_value(value).is_none() && !matches!(value, Expression::Variable(_));
                if last_is_computed {
                    return Err(Diagnostic::error("a run of pointer stores whose last value mwcc latency-hoists needs the scheduler (roadmap)"));
                }
            }
        }
        // A single COMPUTED store to an SDA integer global plus an int return that
        // does NOT read the stored global: mwcc's DAG scheduler interleaves the
        // return-value computation with the store chain; sequential emission
        // diverges. try_dag_store_fill (above) claims every such shape it has
        // vocabulary for, so what reaches here (a division, an unsigned shift)
        // would fall through to the sequential emitter — defer. A return that
        // reads the just-stored global (rand.c) is data-dependent, so mwcc is
        // sequential too — byte-exact on the normal path; let it through.
        if function.guards.is_empty()
            && function.locals.is_empty()
            && !function_makes_call(function)
            && self.behavior.global_addressing == GlobalAddressing::SmallData
            && !matches!(
                function.return_type,
                Type::Void | Type::Float | Type::Double
            )
        {
            if let (
                Some(return_expression),
                [Statement::Store {
                    target: Expression::Variable(name),
                    value,
                }],
            ) = (&function.return_expression, function.statements.as_slice())
            {
                let sda_integer_global = matches!(self.globals.get(name.as_str()), Some(global_type) if !matches!(global_type, Type::Float | Type::Double));
                let leaf_value = constant_value(value).is_some()
                    || matches!(value, Expression::Variable(leaf) if !self.globals.contains_key(leaf.as_str()));
                if sda_integer_global
                    && !leaf_value
                    && count_name_occurrences(return_expression, name) == 0
                {
                    return Err(Diagnostic::error("a computed store scheduled against an independent return needs the DAG scheduler (roadmap)"));
                }
            }
        }
        // A `do { …calls… } while (--counter);` loop: the counter goes in r31
        // (callee-saved), the body branches back, and the decrement-and-test is a
        // single `addic.`/`bne`.
        if self.try_do_while_counter(function)? {
            return Ok(());
        }
        // The C++ ctor/dtor runner: a local pointer walks a NULL-terminated global
        // function-pointer table, calling each entry (`while (*p) { (**p)(); p++; }`).
        if self.try_pointer_walker_call_loop(function)? {
            return Ok(());
        }
        // An empty-body hardware-register poll (`while (__EXIRegs[13] & 1);`):
        // element address materialized once, then load → `rlwinm.`/`cmplwi` → branch back.
        if self.try_emit_busy_wait(function)? {
            return Ok(());
        }
        // A byte-table search loop with one call per candidate, followed by a
        // constant call/return guard chain. The cursor and index occupy r31/r30
        // across every call and all exits share one epilogue.
        if self.try_counted_table_search_with_call_guards(function)? {
            return Ok(());
        }
        // A fixed-count walk over a global object array which reuses each
        // element address across a run of calls. Both the element cursor and
        // integer counter are allocator-owned loop-carried survivors.
        if self.try_indexed_call_sequence_loop(function)? {
            return Ok(());
        }
        // A counted call loop (`for (i = 0; i < N; i++) g(i);`): counter in the r31
        // home, bottom-tested backward branch.
        if self.try_counted_call_loop(function)? {
            return Ok(());
        }
        // A small constant-trip constant fill (`for (i = 0; i < N; i++) A[i] = k;`,
        // N <= 32) unrolls completely — no loop structure at all.
        if self.try_unrolled_fill_loop(function)? {
            return Ok(());
        }
        // A dynamic-bound zero fill (`for (i = 0; i < n; i++) A[i] = 0;`) emits
        // the measured modulo-scheduled 8-way + tail structure.
        if self.try_dynamic_fill_loop(function)? {
            return Ok(());
        }
        // The iota fill (`for (i = 0; i < n; i++) A[i] = i;`): the pipelined
        // 8-way rotation body.
        if self.try_dynamic_iota_loop(function)? {
            return Ok(());
        }
        // A dynamic-bound bare call loop: the counter and bound homes cross the
        // call, so the allocator derives r31/r30.
        if self.try_dynamic_call_loop(function)? {
            return Ok(());
        }
        // A global-flag while loop over a bare call: the flag reloads each
        // iteration (no register crossing).
        if self.try_flag_while_loop(function)? {
            return Ok(());
        }
        // A two-case switch that selects a narrow local, followed by a call tail.
        // The selected value occupies r31 while the incoming argument spills.
        if self.try_switch_assignment_call_tail(function)? {
            return Ok(());
        }
        // A dense table dispatcher whose arms assign one callee result while
        // preserving both the forwarded parameter and result across calls.
        if self.try_switch_call_dispatcher(function)? {
            return Ok(());
        }
        // A two-case member dispatcher forwards two pointer parameters to one
        // call per arm and returns an arm-specific constant through a shared
        // callee-saved epilogue (NW4R TagProcessorBase::Process).
        if self.try_switch_call_return(function)? {
            return Ok(());
        }
        // A function whose body is a single `switch` lowers to the dispatch tree:
        // the comparisons, then the case bodies, then the default (the `default:`
        // arm if present, else the function's trailing `return`). The cases and
        // default each end in their own `blr`, so this owns the whole body.
        if let [Statement::Switch {
            scrutinee,
            arms,
            default,
        }] = function.statements.as_slice()
        {
            let statement_bodied_default =
                matches!(default, Some(body) if body.return_expression().is_none());
            if function.return_type != Type::Void
                && function.guards.is_empty()
                && function.locals.is_empty()
                && !function_makes_call(function)
                && !statement_bodied_default
            {
                let default_expression = default
                    .as_ref()
                    .and_then(|body| body.return_expression())
                    .or(function.return_expression.as_ref())
                    .ok_or_else(|| {
                        Diagnostic::error("a switch with no default needs a trailing return")
                    })?;
                let result = match function.return_type {
                    Type::Float | Type::Double => {
                        return Err(Diagnostic::error(
                            "a floating-point switch result is not supported yet (roadmap)",
                        ))
                    }
                    Type::Void => {
                        return Err(Diagnostic::error(
                            "a void switch is not supported yet (roadmap)",
                        ))
                    }
                    _ => Eabi::general_result().number,
                };
                return self.emit_switch(
                    scrutinee,
                    arms,
                    default_expression,
                    default.is_some(),
                    function.return_type,
                    result,
                );
            }
        }
        // A whole-body `void` function that is a single `switch` with STATEMENT arms
        // (`switch(n){ case V: <stores> break; ... }`): the comparison-tree dispatch, then each
        // arm's statements plus its own `blr` (the arm's `break` is the void function's return).
        // A `default:` statement arm becomes a trailing default block; a MISSING default makes the
        // dispatch's out-of-range branches conditional returns (`bgelr`/`blr`) instead.
        if let [Statement::Switch {
            scrutinee,
            arms,
            default,
        }] = function.statements.as_slice()
        {
            if function.return_type == Type::Void
                && function.guards.is_empty()
                && function.locals.is_empty()
                && !function_makes_call(function)
            {
                match default.as_ref() {
                    Some(mwcc_syntax_trees::ArmBody::Statements(default_statements)) => {
                        return self.emit_statement_switch(
                            scrutinee,
                            arms,
                            Some(default_statements),
                        );
                    }
                    None => {
                        return self.emit_statement_switch(scrutinee, arms, None);
                    }
                    // A value-returning default in a void function is nonsensical; defer.
                    Some(mwcc_syntax_trees::ArmBody::Return(_)) => {}
                }
            }
        }
        // A non-leaf function whose whole body is `if (c) <call>;`: mwcc schedules
        // the condition test (`cmpwi`) into the prologue, between `mflr` and the LR
        // store, then branches forward over the body to the epilogue when false.
        if let [Statement::If {
            condition,
            then_body,
            else_body,
        }] = function.statements.as_slice()
        {
            if function_makes_call(function)
                && function.return_type == Type::Void
                && function.guards.is_empty()
                && else_body.is_empty()
                && !then_body.is_empty()
                // A straight-line body (calls/stores, no nested control flow); a value
                // read across one of its calls would need callee-saving, so defer it.
                && then_body.iter().all(|statement| matches!(statement, Statement::Store { .. } | Statement::Expression(_) | Statement::Assign { .. }))
                && !reads_value_across_call(function)
            {
                self.non_leaf = true;
                self.frame_size = 16;
                // The if's join label advances mwcc's anonymous-`@N` counter by 2.
                self.output.anonymous_label_bump = 2;
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
                let condition_start = self.output.instructions.len();
                let (options, condition_bit) = self.emit_condition_test(condition)?;
                self.schedule_condition_linkage(condition_start);
                let branch_index = self.output.instructions.len();
                self.output
                    .instructions
                    .push(Instruction::BranchConditionalForward {
                        options,
                        condition_bit,
                        target: 0,
                    });
                for statement in then_body {
                    self.emit_statement(statement)?;
                }
                let label = self.output.instructions.len();
                if let Instruction::BranchConditionalForward { target, .. } =
                    &mut self.output.instructions[branch_index]
                {
                    *target = label;
                }
                self.emit_epilogue_and_return();
                return Ok(());
            }
        }
        // A member tested then decremented retains the loaded value and splits
        // its pointer live range before a global-member receiver overwrites r3.
        if self.try_guarded_member_decrement_if_else(function)? {
            return Ok(());
        }
        // A non-leaf `if (c) { then } else { else }` with straight-line bodies: the
        // condition test schedules into the prologue, `beq` jumps to the else body,
        // the then body falls through to an unconditional `b` over the else body to
        // the shared epilogue.
        if let [Statement::If {
            condition,
            then_body,
            else_body,
        }] = function.statements.as_slice()
        {
            if function_makes_call(function)
                && function.guards.is_empty()
                && !then_body.is_empty()
                && !else_body.is_empty()
                && then_body.iter().chain(else_body).all(|statement| matches!(statement, Statement::Store { .. } | Statement::Expression(_) | Statement::Assign { .. }))
                && !reads_value_across_call(function)
                // Void, or an int/unsigned function returning a small CONSTANT — materialized at
                // the join before the epilogue (`join: li r3,C; <epilogue>`); the LR reload hoists
                // between the last call and it, as mwcc does. A non-constant return re-reads a value
                // across the call and defers.
                && (function.return_type == Type::Void
                    || (matches!(function.return_type, Type::Int | Type::UnsignedInt)
                        && function.return_expression.as_ref().and_then(|expression| constant_value(expression)).is_some_and(|value| i16::try_from(value).is_ok())))
            {
                self.non_leaf = true;
                self.frame_size = 16;
                // The else branch and join label advance mwcc's anonymous-`@N` counter.
                self.output.anonymous_label_bump = 3;
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
                let condition_start = self.output.instructions.len();
                let (options, condition_bit) = self.emit_condition_test(condition)?;
                self.schedule_condition_linkage(condition_start);
                let branch_to_else = self.output.instructions.len();
                self.output
                    .instructions
                    .push(Instruction::BranchConditionalForward {
                        options,
                        condition_bit,
                        target: 0,
                    });
                for statement in then_body {
                    self.emit_statement(statement)?;
                }
                let branch_to_join = self.output.instructions.len();
                self.output
                    .instructions
                    .push(Instruction::Branch { target: 0 });
                let else_label = self.output.instructions.len();
                if let Instruction::BranchConditionalForward { target, .. } =
                    &mut self.output.instructions[branch_to_else]
                {
                    *target = else_label;
                }
                for statement in else_body {
                    self.emit_statement(statement)?;
                }
                let join_label = self.output.instructions.len();
                if let Instruction::Branch { target } =
                    &mut self.output.instructions[branch_to_join]
                {
                    *target = join_label;
                }
                // A non-void function materializes its constant return beside the LR reload:
                // mainline uses `lwz r0,20; li r3,C`, while build 163 uses `li r3,C; lwz r0,20`.
                // The reload-hoist pass bails on the join's forward branches, so emit this
                // schedule explicitly. This handler builds a plain 16-byte frame with no
                // callee-saved GPRs, leaving only mtlr / teardown after the two scheduled ops.
                if let Some(constant) = function
                    .return_expression
                    .as_ref()
                    .filter(|_| function.return_type != Type::Void)
                    .and_then(|expression| constant_value(expression))
                {
                    self.emit_non_leaf_constant_join_epilogue(constant);
                } else {
                    self.emit_epilogue_and_return();
                }
                return Ok(());
            }
        }
        // A LEAF if/else diamond (both arms store) with a return continuation — the JOIN
        // (materialized return) and TWO-EXIT (return value already in r3) forms. See
        // body/if_else.rs.
        if self.try_leaf_ifelse_diamond(function)? {
            return Ok(());
        }
        // A non-leaf function led by `if (c) { …calls…; return X; }` with a
        // continuation that supplies the other exit: mwcc schedules the condition
        // test into the prologue, the early return materializes X and branches to a
        // SHARED epilogue, and the continuation falls into that same epilogue.
        if self.try_non_leaf_if_first_early_return(function)? {
            return Ok(());
        }
        // A shared member-store value, narrow guard, and guarded call form one
        // measured scheduling region. It owns the linkage frame as well as the
        // statements, so claim it before the generic non-leaf prologue below.
        if self.try_leading_store_guarded_call(function)? {
            return Ok(());
        }
        // A function that calls is non-leaf: save the link register using the
        // selected generation's linkage convention before doing anything else.
        let mut lr_store_index: Option<usize> = None;
        if function_makes_call(function) {
            if !function.guards.is_empty() {
                // `if (call()) return C; ... return D;` — a sequence of
                // call-tested constant exits sharing one LR-only epilogue.
                if self.try_call_condition_return_chain(function)? {
                    return Ok(());
                }
                if self.try_float_call_short_circuit_guard(function)? {
                    return Ok(());
                }
                // `if (b) return call(); return DEFAULT;` — a guarded early return
                // whose value is a call (no callee-saved register needed).
                if self.try_guarded_call_return(function)? {
                    return Ok(());
                }
                return Err(Diagnostic::error(
                    "calls combined with guards not yet supported",
                ));
            }
            // `while (n) { call(…n…); n--; }` — a counter kept in r31 across a
            // call-containing loop, updated in place.
            if self.try_virtual_collection_scan(function)? {
                return Ok(());
            }
            if self.try_callee_saved_call_loop(function)? {
                return Ok(());
            }
            if self.try_call_live_counter_loop(function)? {
                return Ok(());
            }
            // (guard-less call handlers continue below)
            // `*a = g(); *b = h();` — 2–4 output pointers saved in r31/r30/… across their calls.
            // Runs before the general callee-saved path, which would otherwise emit the stores
            // through the raw (clobbered) argument registers and defer/miscompile.
            if self.try_stores_through_pointers(function)? {
                return Ok(());
            }
            // `int t = gi; g(); return t;` — a memory-loaded local carried across calls in r31.
            if self.try_callee_saved_memory_local(function)? {
                return Ok(());
            }
            // A guarded dirty-bit dispatcher, flush test, and fixed-width port writes. Three
            // parameters plus the dirty word survive calls in a linkage-first r28..r31 frame.
            if self.try_guarded_bitmask_call_sequence(function)? {
                return Ok(());
            }
            // `flags = state->dirty; if (flags & A) callA(); ...; state->dirty = 0;` — one
            // memory-loaded bitmask retained in r31 across a chain of conditional SDK calls.
            if self.try_callee_saved_bitmask_call_chain(function)? {
                return Ok(());
            }
            // `F t = gf; if (!t) return; t();` — a guarded call through a global fn-pointer.
            if self.try_guarded_global_pointer_call(function)? {
                return Ok(());
            }
            // Parameters live across the call go in callee-saved registers (r31
            // descending), saved in the prologue and reloaded in the epilogue.
            if self.try_frsqrte_sqrt(function)? {
                return Ok(());
            }
            if self.try_float_callee_saved(function)? {
                return Ok(());
            }
            if self.try_callee_saved(function)? {
                return Ok(());
            }
            if self.try_callee_saved_call_result(function)? {
                return Ok(());
            }
            // `int x = g(); int y = h(); return x OP y;` — two call-result locals with NO
            // trailing call: only the first parks in a callee-saved register, the second
            // stays in r3. See callee_saved/combine.rs.
            if self.try_callee_saved_two_call_result_combine(function)? {
                return Ok(());
            }
            // `int x = g(a); return x OP a;` — a call-result local combined with the
            // parameter that crossed the call.
            if self.try_callee_saved_result_param_combine(function)? {
                return Ok(());
            }
            // `int x = g(a); return x + a + b;` / `int x = g(a); int y = g(x);
            // return y + a + x;` — a call result added to TWO saved values: mwcc
            // reassociates, parking the result in r0 and combining the homes first.
            if self.try_callee_saved_result_park_combine(function)? {
                return Ok(());
            }
            // `int x = <expr>; int y = g(x); return y OP x;` — a computed local
            // crossing the call that consumes it, combined with the result.
            if self.try_callee_saved_computed_then_call(function)? {
                return Ok(());
            }
            // `*p = g();` — a call's result stored through a pointer parameter saved in r31.
            if self.try_store_call_through_pointer(function)? {
                return Ok(());
            }
            if self.try_callee_saved_computed_local(function)? {
                return Ok(());
            }
            // A parameter passed to several calls in turn (`g(x); h(x);`) — saved in r31,
            // the first call uses the incoming register, later calls restore from r31.
            if self.try_callee_saved_call_args(function)? {
                return Ok(());
            }
            // `return f(...) + x;` — a live parameter combined with a call's result in the return.
            if self.try_callee_saved_call_combine(function)? {
                return Ok(());
            }
            // `p(x); q(y);` — two params passed to two calls in turn; the later param is preserved.
            if self.try_callee_saved_call_sequence(function)? {
                return Ok(());
            }
            // `x = f(); g(x); h(x);` — a call result live across the calls that consume it.
            if self.try_callee_saved_result_call_sequence(function)? {
                return Ok(());
            }
            // `x = f(); g(<literals>); return x;` — a call result live across one call, returned.
            if self.try_callee_saved_result_across_call_return(function)? {
                return Ok(());
            }
            // `x = parameter->member; g(x, integer, float);` — a computed
            // local that dies as the first argument of its only call.
            if self.try_computed_local_call_forward(function)? {
                return Ok(());
            }
            // A producer result forwarded as the third argument of a trace-like
            // consumer dies at that call; stage it directly into r5 while the
            // consumer's split string address is in flight.
            if self.try_result_trace_forward_constant_return(function)? {
                return Ok(());
            }
            // `x = f(parameters...); g(x, constants...);` — all parameters die
            // in f and x remains in r3 for g's first argument, so neither value
            // needs a callee-saved home. The mixed GPR/FPR tail lives separately
            // from the older integer-only, parameterless result-feed schedule.
            if self.try_result_call_forward_with_live_ins(function)? {
                return Ok(());
            }
            // `x = f(); g(…, x, …);` void — the result feeds the next call and dies (no home).
            if self.try_result_feeds_call(function)? {
                return Ok(());
            }
            // `g(x); return x OP y;` — two params both live across one call, combined in the return.
            if self.try_callee_saved_param_pair_combine(function)? {
                return Ok(());
            }
            // `g(a); h(b); return a OP b;` — two params passed to two calls in turn, then combined.
            if self.try_callee_saved_call_sequence_combine(function)? {
                return Ok(());
            }
            // `void f(int a){ g(); h(a); }` — one param live across leading bare calls, then passed.
            if self.try_callee_saved_param_across_calls(function)? {
                return Ok(());
            }
            // `void f(struct S *s){ s->cb(7); }` — a bare indirect call with constant arguments,
            // through a memory-resident function pointer (the base collides with the first arg reg).
            if self.try_indirect_call_with_constant_args(function)? {
                return Ok(());
            }
            // `h(g(), p)` — a live parameter passed alongside a nested call that produces another arg.
            if self.try_callee_saved_nested_call_arg(function)? {
                return Ok(());
            }
            // `return f() OP g();` — two call results combined in the return.
            if self.try_callee_saved_two_call_combine(function)? {
                return Ok(());
            }
            // `if (status() == 0) { object->field = ...; call(); }` — the
            // condition's call clobbers a live-in used only by the selected arm.
            // The semantic owner emits a virtual survivor and lets the shared
            // allocator choose its callee-saved home.
            if self.try_call_condition_live_in_if(function)? {
                return Ok(());
            }
            // Byte-exact-or-defer: a value (parameter or register local) read after a
            // call is read from a register the call clobbered. mwcc preserves it in a
            // callee-saved register (r31…) — multi-value/local cases are the next
            // step; until then DEFER rather than emit a read of the clobbered register.
            // `if (b) call(); return a;` — a value live across a CONDITIONAL call
            // (the #20/#21 intersection). Handled generally before the defer.
            if self.try_callee_saved_conditional_call(function)? {
                return Ok(());
            }
            // The void sibling: `if (cond) { calls } *p = <const>;` — the store's base
            // parameter is live across the conditional call. See callee_saved/conditional.rs.
            if self.try_callee_saved_conditional_call_then_store(function)? {
                return Ok(());
            }
            // `int x = G; call(); G2 = x;` — the first fully general-allocator
            // crossing shape (virtual home, callee-saved pool, frame builder).
            if self.try_callee_saved_global_round_trip(function)? {
                return Ok(());
            }
            // SDK callback registrars: swap a callback global while interrupts
            // are disabled, returning the old callback. Both word values cross
            // calls and are colored by the virtual-register allocator.
            if self.try_interrupt_protected_global_swap(function)? {
                return Ok(());
            }
            // `state=enter(); value=EXPR; leave(state); return value;` — r3
            // carries state while EXPR is parked in a callee-saved virtual.
            if self.try_computed_value_between_calls(function)? {
                return Ok(());
            }
            // `state=enter(); fixed_regs[k] = (fixed_regs[k]&mask)|param_bits; ...;
            // leave(state);` — saved parameters feed a latency-scheduled hardware-register
            // programming run while r3 retains the critical-section state.
            if self.try_interrupt_protected_fixed_rmw(function)? {
                return Ok(());
            }
            // Critical-section allocator bump: preserve an input and the old
            // global result while updating a pointer cursor and free count.
            if self.try_interrupt_protected_allocator_bump(function)? {
                return Ok(());
            }
            // Build 163's asynchronous state callback interleaves its outer
            // condition with the linkage prologue and rejoins switch/retry arms.
            if self.try_async_state_callback(function)? {
                return Ok(());
            }
            // General structured body with values spanning conditional calls:
            // assign virtual callee-saved homes once, then lower its forward
            // branches through the ordinary expression/store emitters.
            if self.try_callee_saved_structured_body(function)? {
                return Ok(());
            }
            // SDK list walks which snapshot `next` before conditionally calling
            // on the current node. The successor is a genuine loop-carried
            // callee-saved value; keep this beside the other allocator owners.
            if self.try_pointer_state_call_loop(function)? {
                return Ok(());
            }
            // A range-guarded global-array element consumed by several calls
            // has one cross-call address survivor and a source-ordered false
            // edge for every guard term.
            if self.try_guarded_indexed_call_sequence(function)? {
                return Ok(());
            }
            // Counted resource searches keep a status, counter, acquired
            // object, and two output pointers live across their call chain.
            // With inline multiple saves, the allocator colors one dense GPR
            // region and this semantic owner emits the measured loop schedule.
            if self.try_counted_resource_search(function)? {
                return Ok(());
            }
            // A bounded byte-buffer append keeps its error, transfer length,
            // and buffer base in r31..r29 across the conditional copy call.
            if self.try_bounded_buffer_append(function)? {
                return Ok(());
            }
            if self.try_bounded_buffer_read(function)? {
                return Ok(());
            }
            if reads_value_across_call(function) {
                return Err(Diagnostic::error(format!(
                    "a value live across a call needs the callee-saved register allocator (roadmap; function '{}')",
                    function.name
                )));
            }
            lr_store_index = Some(self.emit_plain_nonleaf_prologue());
        }

        // A shared-value member-store run followed by a fixed-address pointer
        // clear has a measured cross-statement owner.
        if self.try_leading_store_guard(function)? {
            return Ok(());
        }
        // A leading store (or store run) before a trailing `if` needs mwcc's cross-statement
        // scheduler: it hoists the if's condition test as early as possible — into the leading
        // store's value-materialize latency gap (`li r0,1; cmpwi; stw r0,g; beqlr; …`) or to the
        // front. The sequential emission below instead emits the store fully, then the test — a
        // DIFFERS — so defer this shape. (A whole-body store run, or a whole-body trailing `if`,
        // are handled byte-exactly by the store-fill matchers above.)
        if let [leading @ .., Statement::If { .. }] = function.statements.as_slice() {
            if !leading.is_empty()
                && leading
                    .iter()
                    .all(|statement| matches!(statement, Statement::Store { .. }))
            {
                return Err(Diagnostic::error(format!(
                    "a leading store before a trailing if needs the cross-statement scheduler (roadmap; function '{}')",
                    function.name
                )));
            }
        }

        // A leading early-return if whose continuation MATERIALIZES store values (a
        // constant/computed value, or several stores) schedules the return value between
        // the materialization and the store (`li r0,5; li r3,0; stw r0`), or interleaves
        // a store batch — the sequential emission below would emit the store first, a
        // byte-DIFF. The verified single-constant-store form is handled by
        // try_ordered_early_return_branch; everything else here defers. (A store of a
        // plain register value needs no materialization and stays — verified.)
        if let [Statement::If { then_body, .. }, continuation @ ..] = function.statements.as_slice()
        {
            if matches!(then_body.as_slice(), [Statement::Return(_)]) {
                let store_count = continuation
                    .iter()
                    .filter(|statement| matches!(statement, Statement::Store { .. }))
                    .count();
                let materializing_store = continuation.iter().any(|statement| {
                    matches!(statement, Statement::Store { value, .. }
                        if !matches!(value, Expression::Variable(name) if self.locations.contains_key(name.as_str())))
                });
                // A computed-index GLOBAL-ARRAY target materializes its ADDRESS
                // (lis/slwi/addi) even for a register value — with a live return, mwcc
                // keeps the base out of the index register and interleaves the return
                // (`addi r5,r5; li r3,0; stwx r4,r5,r0`), which the sequential emission
                // below does not model. (A pointer-parameter target needs no address
                // build and stays — verified.)
                let address_materializing_store = continuation.iter().any(|statement| {
                    matches!(statement, Statement::Store { target: Expression::Index { base, index }, .. }
                        if matches!(base.as_ref(), Expression::Variable(name) if self.globals.contains_key(name.as_str()))
                            && constant_value(index).is_none())
                });
                if store_count >= 2 || materializing_store || address_materializing_store {
                    return Err(Diagnostic::error(
                        "an early-return continuation that materializes store values needs the store/return scheduler (roadmap)",
                    ));
                }
            }
        }

        // Body statements (stores, calls) run first.
        let statements_start = self.output.instructions.len();
        let statement_count = function.statements.len();
        let scheduled_global_store_return = self.global_constant_store_return_plan(function);
        let statement_end = scheduled_global_store_return
            .as_ref()
            .map_or(statement_count, |plan| plan.statement_start);
        for (index, statement) in function.statements[..statement_end].iter().enumerate() {
            // A trailing `if (c) { body }` in a leaf void function: the false path
            // is the function exit, so it is a conditional return, then the body,
            // then the normal `blr`. (Non-leaf needs a forward branch to the
            // epilogue, and a non-final if needs to skip forward — both deferred.)
            if let Statement::If {
                condition,
                then_body,
                else_body,
            } = statement
            {
                // A leaf if whose then-body is at most one statement then an early
                // `return`, with a continuation after it (more statements or the
                // trailing return): forward-branch over the body, the return is an
                // exit, and the branch lands on the continuation. Two or more
                // leading statements (constant stores mwcc would interleave) need
                // the scheduler. With no continuation (a trailing void if) the
                // false path is the immediate exit, which is a `beqlr` form — that
                // and the multi-statement case defer.
                let has_continuation =
                    index + 1 < statement_count || function.return_expression.is_some();
                // A trailing void `if (c) { stmt; return; }` (nothing after): the
                // `return;` coincides with the function exit, so drop it and use
                // the conditional-return (`beqlr`) form of a plain trailing if.
                if !function_makes_call(function)
                    && else_body.is_empty()
                    && !has_continuation
                    && function.return_type == Type::Void
                    && then_body.len() == 2
                    && matches!(then_body.last(), Some(Statement::Return(None)))
                {
                    self.emit_trailing_if(condition, &then_body[..1], else_body, false)?;
                    continue;
                }
                // A trailing-void, no-else if-BLOCK of two-plus REGISTER-VALUED stores (each value
                // already in a register — nothing to materialize or schedule): the conditional
                // return then the stores in source order. A constant/global/computed value needs the
                // batch scheduler, so emit_trailing_if defers those.
                if !function_makes_call(function)
                    && else_body.is_empty()
                    && index + 1 == statement_count
                    && function.return_type == Type::Void
                    && then_body.len() >= 2
                    && then_body.iter().all(|inner| matches!(inner,
                        Statement::Store { value: Expression::Variable(name), .. } if self.locations.contains_key(name.as_str())))
                {
                    self.emit_trailing_if(condition, then_body, else_body, false)?;
                    continue;
                }
                if !function_makes_call(function)
                    && else_body.is_empty()
                    && then_body.len() <= 2
                    && has_continuation
                    && matches!(then_body.last(), Some(Statement::Return(_)))
                    // A store before a VALUE return must be INTERLEAVED with the return-value
                    // computation the way mwcc's scheduler does (`li r0,V; li r3,R; stw r0`, not
                    // `li r0,V; stw r0; li r3,R`) — that needs the keystone scheduler (#20), so
                    // defer it. A valueless `return;` has no value to interleave (store + bare
                    // epilogue is byte-exact), and a value-tracked Assign emits nothing here, so
                    // both of those stay byte-exact.
                    && (matches!(then_body.last(), Some(Statement::Return(None)))
                        || then_body[..then_body.len() - 1].iter().all(|statement| matches!(statement, Statement::Assign { .. })))
                {
                    self.emit_if_early_return(condition, then_body, function.return_type)?;
                    continue;
                }
                // Single-statement leaf if-blocks. A multi-statement body needs the
                // instruction scheduler, and a non-leaf if needs the cmpwi scheduled
                // into the prologue — both defer for now.
                if then_body.len() == 1 && !function_makes_call(function) {
                    let trailing_void =
                        index + 1 == statement_count && function.return_type == Type::Void;
                    if trailing_void {
                        // The false path is the function exit (or the else / else-if):
                        // a conditional return, or a branch into the else chain.
                        self.emit_trailing_if(condition, then_body, else_body, false)?;
                        continue;
                    }
                    if else_body.is_empty() {
                        // A conditional store to a global that the very NEXT statement
                        // unconditionally overwrites is a DEAD store: mwcc drops the whole `if`
                        // (the condition has no side effect here — this branch is call-free) and
                        // emits only the final store. We do not do that dead-store elimination, so
                        // emitting both stores faithfully would diverge — defer instead.
                        fn store_target(statement: &Statement) -> Option<&str> {
                            match statement {
                                Statement::Store {
                                    target: Expression::Variable(name),
                                    ..
                                } => Some(name.as_str()),
                                _ => None,
                            }
                        }
                        if let Some(dead) = store_target(&then_body[0]) {
                            if function.statements.get(index + 1).and_then(store_target)
                                == Some(dead)
                            {
                                return Err(Diagnostic::error("a dead conditional store (overwritten by the next statement) needs dead-store elimination (roadmap)"));
                            }
                        }
                        // The false path skips the body: forward branch.
                        self.emit_if_forward(condition, then_body)?;
                        continue;
                    }
                }
                // A non-trailing multi-store if-BLOCK that is the FIRST statement of a void body and
                // is followed by exactly one trailing store: `cmpwi; beq cont; <then run>; cont:
                // <trailing store>; blr`. The if-first restriction avoids the leading-store-before-if
                // scheduler; the single trailing store is what the loop emits byte-exactly next. A
                // register-valued then-run stores sequentially, a constant one materializes batched.
                if !function_makes_call(function)
                    && else_body.is_empty()
                    && function.return_type == Type::Void
                    && index == 0
                    && statement_count == 2
                    && matches!(function.statements.get(1), Some(Statement::Store { .. }))
                    && then_body.len() >= 2
                {
                    let then_plan = self.constant_store_run_plan(then_body);
                    if then_plan.is_some() || self.store_run_arm_registers(then_body) {
                        let (options, condition_bit) = self.emit_condition_test(condition)?;
                        let branch_index = self.output.instructions.len();
                        self.output
                            .instructions
                            .push(Instruction::BranchConditionalForward {
                                options,
                                condition_bit,
                                target: 0,
                            });
                        match then_plan {
                            Some(plan) => self.emit_constant_store_run(then_body, plan)?,
                            None => {
                                for statement in then_body {
                                    self.emit_statement(statement)?;
                                }
                            }
                        }
                        let label = self.output.instructions.len();
                        if let Instruction::BranchConditionalForward { target, .. } =
                            &mut self.output.instructions[branch_index]
                        {
                            *target = label;
                        }
                        continue;
                    }
                }
            }
            self.emit_statement(statement)?;
        }
        let return_start = self.output.instructions.len();

        // Hoist a leading register move from the body's statements (a call's argument
        // setup) into the prologue's mflr->LR-store slot.
        if !self.schedule_leading_member_store_call() {
            self.hoist_leading_arg_moves(lr_store_index);
        }

        if let Some(plan) = scheduled_global_store_return {
            self.emit_global_constant_store_return_plan(plan)?;
            self.emit_epilogue_and_return();
            return Ok(());
        }

        // A `void` function ends after its statements.
        if function.return_type == Type::Void {
            self.emit_epilogue_and_return();
            return Ok(());
        }

        let result = match function.return_type {
            Type::Float | Type::Double => Eabi::float_result().number,
            _ => Eabi::general_result().number,
        };
        // A non-void function may FALL OFF THE END (C89; strikers alloc's
        // FORCE_DONT_INLINE stubs) — mwcc emits a bare blr, r3 undefined.
        let Some(return_expression) = function.return_expression.as_ref() else {
            if function.guards.is_empty() {
                self.emit_epilogue_and_return();
                return Ok(());
            }
            return Err(Diagnostic::error(
                "a non-void function needs a return value",
            ));
        };

        if !function.guards.is_empty() {
            // Guard + single value-tracked local, zero-select: `int x = a+1; if (c) return 0;
            // return x;` (or `if (c) return x; return 0;`). mwcc materializes the local in
            // the result register but SCHEDULES the materialization into the select's
            // neg->or latency slot — `neg r0,c; addi r3,a,1; or r0,r0,c; srawi r0,31; and/
            // andc r3,r3,r0` (the addi AFTER the leading neg). Emit that interleave directly:
            // leading neg, the local, then the mask combine. Restricted to a single-op
            // integer local, a leaf condition, no statements, and exactly one arm the
            // constant 0 (the other the local).
            if let ([local], [guard]) = (function.locals.as_slice(), function.guards.as_slice()) {
                let zero_is_then = matches!(guard.value, Expression::IntegerLiteral(0));
                let zero_is_else = matches!(return_expression, Expression::IntegerLiteral(0));
                let local_is_other = (zero_is_then
                    && matches!(return_expression, Expression::Variable(name) if *name == local.name))
                    || (zero_is_else
                        && matches!(&guard.value, Expression::Variable(name) if *name == local.name));
                let condition_register =
                    leaf_name(&guard.condition).and_then(|name| self.lookup_general(name));
                let initializer = local.initializer.as_ref();
                if local_is_other
                    && function.statements.is_empty()
                    && initializer.is_some_and(|init| self.is_single_op_register_value(init))
                    && class_of(local.declared_type)? == ValueClass::General
                {
                    if let (Some(condition_register), Some(initializer)) =
                        (condition_register, initializer)
                    {
                        if self.behavior.integer_select_style
                            == mwcc_versions::IntegerSelectStyle::BranchPreserving
                        {
                            let (options, condition_bit) =
                                self.emit_condition_test(&guard.condition)?;
                            self.evaluate(initializer, local.declared_type, result)?;
                            self.output.instructions.push(
                                Instruction::BranchConditionalToLinkRegister {
                                    options: if zero_is_then { options } else { options ^ 8 },
                                    condition_bit,
                                },
                            );
                            self.load_integer_constant(result, 0);
                            self.output
                                .instructions
                                .push(Instruction::BranchToLinkRegister);
                            return Ok(());
                        }
                        self.output.instructions.push(Instruction::Negate {
                            d: GENERAL_SCRATCH,
                            a: condition_register,
                        });
                        self.evaluate(initializer, local.declared_type, result)?;
                        self.output.instructions.push(Instruction::Or {
                            a: GENERAL_SCRATCH,
                            s: GENERAL_SCRATCH,
                            b: condition_register,
                        });
                        self.output
                            .instructions
                            .push(Instruction::ShiftRightAlgebraicImmediate {
                                a: GENERAL_SCRATCH,
                                s: GENERAL_SCRATCH,
                                shift: 31,
                            });
                        self.output.instructions.push(if zero_is_then {
                            Instruction::AndComplement {
                                a: result,
                                s: result,
                                b: GENERAL_SCRATCH,
                            }
                        } else {
                            Instruction::And {
                                a: result,
                                s: result,
                                b: GENERAL_SCRATCH,
                            }
                        });
                        self.output
                            .instructions
                            .push(Instruction::BranchToLinkRegister);
                        return Ok(());
                    }
                }
            }
            if !function.locals.is_empty() {
                return Err(Diagnostic::error(
                    "locals combined with guards not yet supported",
                ));
            }
            // mwcc lowers a single guard as a select (working-register form) but a
            // chain of guards as separate return blocks.
            if let [guard] = function.guards.as_slice() {
                if constant_value(&guard.value) == Some(1)
                    && constant_value(return_expression) == Some(0)
                {
                    if let Expression::Binary {
                        operator,
                        left,
                        right,
                    } = &guard.condition
                    {
                        if self.try_emit_unsigned_narrow_less_constant(
                            *operator, left, right, result,
                        )? {
                            self.output
                                .instructions
                                .push(Instruction::BranchToLinkRegister);
                            return Ok(());
                        }
                    }
                }
                // A logical (&&/||) condition short-circuits straight into the two return
                // blocks rather than computing the operator as a 0/1 value.
                if self.try_emit_short_circuit_guard(
                    &guard.condition,
                    &guard.value,
                    return_expression,
                    result,
                )? {
                    return Ok(());
                }
                // `if (c) return X; return X` is degenerate: both paths return the same
                // value, and mwcc keeps the dead condition test then a single `blr`. Defer
                // rather than emit a spurious conditional return for the matching arms.
                if let (Expression::Variable(value_name), Expression::Variable(return_name)) =
                    (&guard.value, return_expression)
                {
                    if value_name == return_name {
                        return Err(Diagnostic::error("a guard whose value equals the fall-through return is degenerate (roadmap)"));
                    }
                }
                self.account_folded_float_guard_labels(&guard.condition);
                if self.try_legacy_tracked_guard_return(function, return_expression, result)? {
                    return Ok(());
                }
                // A null-guarded dereference (`if (!p) return CONST; return *p;` or the mirror
                // `if (p) return *p; return CONST;`) cannot fold branchless — dereferencing null is
                // unsafe — so mwcc branches on `p == 0` to the cold constant with the access in the
                // fall-through: `cmplwi p,0; beq COLD; <hot access>; blr; COLD: li CONST; blr`.
                if let Some((pointer, hot, cold)) = guarded_null_dereference(
                    &guard.condition,
                    &guard.value,
                    return_expression,
                    function.return_type,
                ) {
                    if self.emit_guarded_null_access(
                        &guard.condition,
                        pointer,
                        hot,
                        cold,
                        function.return_type,
                        result,
                    )? {
                        return Ok(());
                    }
                }
                let select = if_select(
                    &guard.condition,
                    &guard.value,
                    return_expression,
                    mwcc_syntax_trees::ConditionalOrigin::IfReturns,
                    self.behavior.integer_select_style
                        == mwcc_versions::IntegerSelectStyle::Branchless,
                );
                // ATTEMPT the select; a fall-through outside its vocabulary (a
                // table load, a cast) uses mwcc's early-return BRANCH instead
                // (measured) — roll back and take the guard-sequence path.
                let instructions_before = self.output.instructions.len();
                let relocations_before = self.output.relocations.len();
                let virtuals_before = self.next_virtual;
                let bump_before = self.output.anonymous_label_bump;
                let labels_before = self.labels.checkpoint();
                match self.evaluate_tail(&select, function.return_type, result) {
                    Ok(()) => {
                        self.output
                            .instructions
                            .push(Instruction::BranchToLinkRegister);
                        return Ok(());
                    }
                    Err(_) => {
                        self.output.instructions.truncate(instructions_before);
                        self.output.relocations.truncate(relocations_before);
                        self.next_virtual = virtuals_before;
                        self.output.anonymous_label_bump = bump_before;
                        self.labels.rollback(labels_before);
                    }
                }
            }
            return self.emit_guard_sequence(
                &function.guards,
                return_expression,
                function.return_type,
                result,
            );
        }

        // The FLOAT DAG arm claims double multiply-add trees (including
        // named double locals — the window-top tier) for the frozen float
        // models before the single-scratch evaluator paths.
        if !self.try_float_dag_return(function)? {
            match function.locals.as_slice() {
                [] => self.evaluate_tail(return_expression, function.return_type, result)?,
                [local] => self.evaluate_single_local(
                    local,
                    return_expression,
                    function.return_type,
                    result,
                )?,
                _ => {
                    return Err(Diagnostic::error(
                        "multiple locals need the full register allocator (roadmap M1)",
                    ))
                }
            }
        }
        self.schedule_legacy_single_pointer_store_return(function, statements_start, return_start);
        // A return value that is itself a call (`return h(p->a, p->b);`) emits its
        // argument setup here, after the body loop's hoist ran — so hoist again now.
        self.hoist_leading_arg_moves(lr_store_index);
        // A `float` function returning a double-precision value rounds to single
        // (`frsp`) before returning, as mwcc does.
        if function.return_type == Type::Float && self.is_double_value(return_expression) {
            self.output.instructions.push(Instruction::RoundToSingle {
                d: result,
                b: result,
            });
        }
        self.emit_epilogue_and_return();
        Ok(())
    }

    pub(crate) fn emit_epilogue_and_return(&mut self) {
        if self.behavior.frame_convention == FrameConvention::LinkageFirst && self.non_leaf {
            if self.callee_saved.is_empty()
                && self.behavior.plain_linkage_epilogue_style
                    == PlainLinkageEpilogueStyle::StackRestoreBeforeReload
            {
                self.output.instructions.push(Instruction::AddImmediate {
                    d: 1,
                    a: 1,
                    immediate: self.frame_size,
                });
                self.output.instructions.push(Instruction::LoadWord {
                    d: 0,
                    a: 1,
                    offset: 4,
                });
                self.output
                    .instructions
                    .push(Instruction::MoveToLinkRegister { s: 0 });
                self.output
                    .instructions
                    .push(Instruction::BranchToLinkRegister);
                return;
            }
            self.output.instructions.push(Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: self.frame_size + 4,
            });
            for (index, &register) in self.callee_saved.iter().enumerate() {
                let offset = self.frame_size - 4 * (index as i16 + 1);
                self.output.instructions.push(Instruction::LoadWord {
                    d: register,
                    a: 1,
                    offset,
                });
            }
            self.output.instructions.push(Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: self.frame_size,
            });
            self.output
                .instructions
                .push(Instruction::MoveToLinkRegister { s: 0 });
            self.output
                .instructions
                .push(Instruction::BranchToLinkRegister);
            return;
        }
        let reload_saved_gprs = |generator: &mut Self| {
            for (index, &register) in generator.callee_saved.iter().enumerate() {
                let offset = generator.frame_size - 4 * (index as i16 + 1);
                generator.output.instructions.push(Instruction::LoadWord {
                    d: register,
                    a: 1,
                    offset,
                });
            }
        };
        if self.epilogue_lr_before_gprs && self.non_leaf {
            // Multi-pointer store sink: the saved LR reloads FIRST, then every callee-saved
            // GPR highest-first, then `mtlr` (`lwz r0,20; lwz r31,12; lwz r30,8; mtlr`).
            self.output.instructions.push(Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: self.frame_size + 4,
            });
            reload_saved_gprs(self);
            self.output
                .instructions
                .push(Instruction::MoveToLinkRegister { s: 0 });
        } else if self.epilogue_lr_first && self.non_leaf {
            // Store-sink callee-saved: mwcc reloads all saved GPRs except the LOWEST, then
            // the saved LR, then the lowest GPR (count==1: `lwz r0; lwz r31`; count==2: `lwz
            // r31; lwz r0; lwz r30`). A register-death schedule this reproduces for one or
            // two saved values; three or more reschedule it (the sink restricts to <= 2).
            let last = self.callee_saved.len().saturating_sub(1);
            for (index, &register) in self.callee_saved.iter().enumerate() {
                if index == last {
                    continue;
                }
                let offset = self.frame_size - 4 * (index as i16 + 1);
                self.output.instructions.push(Instruction::LoadWord {
                    d: register,
                    a: 1,
                    offset,
                });
            }
            self.output.instructions.push(Instruction::LoadWord {
                d: 0,
                a: 1,
                offset: self.frame_size + 4,
            });
            if let Some(&register) = self.callee_saved.last() {
                let offset = self.frame_size - 4 * (last as i16 + 1);
                self.output.instructions.push(Instruction::LoadWord {
                    d: register,
                    a: 1,
                    offset,
                });
            }
            self.output
                .instructions
                .push(Instruction::MoveToLinkRegister { s: 0 });
        } else {
            // Reload callee-saved registers (highest first, from the top of the frame)
            // before the saved-LR reload, so that reload stays directly before `mtlr`
            // where the hoist pass finds it and issues it right after the last call.
            reload_saved_gprs(self);
            if self.non_leaf {
                self.output.instructions.push(Instruction::LoadWord {
                    d: 0,
                    a: 1,
                    offset: self.frame_size + 4,
                });
                self.output
                    .instructions
                    .push(Instruction::MoveToLinkRegister { s: 0 });
            }
        }
        if self.frame_size != 0 {
            self.output.instructions.push(Instruction::AddImmediate {
                d: 1,
                a: 1,
                immediate: self.frame_size,
            });
        }
        self.output
            .instructions
            .push(Instruction::BranchToLinkRegister);
    }

    /// Emit a body statement.
    pub(crate) fn emit_statement(&mut self, statement: &Statement) -> Compilation<()> {
        match statement {
            Statement::Break | Statement::Continue | Statement::Goto(_) | Statement::Label(_) => {
                Err(Diagnostic::error(
                    "a break/continue/goto/label statement is not lowered here yet (captures only)",
                ))
            }
            Statement::Store { target, value } => self.emit_store(target, value),
            Statement::Expression(Expression::Call { name, arguments }) => {
                self.emit_call(name, arguments, None, false)
            }
            Statement::Expression(Expression::VirtualCall {
                object,
                vptr_offset,
                slot_offset,
                variadic,
                arguments,
                ..
            }) => self.emit_virtual_call(
                object,
                *vptr_offset,
                *slot_offset,
                *variadic,
                arguments,
                None,
                false,
            ),
            Statement::Expression(Expression::ConstructedNew {
                allocation,
                allocation_size,
                constructor,
                arguments,
            }) => self.emit_discarded_constructed_new(
                allocation,
                *allocation_size,
                constructor,
                arguments,
            ),
            Statement::Expression(expression @ Expression::Comma { .. }) => {
                self.emit_discarded_comma_sequence(expression)
            }
            // Assignment is an expression in C and C++. The parser normally
            // canonicalizes a discarded scalar assignment to `Statement::Store`,
            // but semantic inline expansion can introduce typed assignment
            // leaves after that statement-level pass (notably scalarized
            // aggregate copies). Their discarded result needs the same store.
            Statement::Expression(Expression::Assign { target, value }) => {
                self.emit_store(target, value)
            }
            // A bare indirect-call statement `(*s->fp)()` / `(**pp)()`: load the
            // callee pointer into r12, then `mtctr r12; bctrl`. Only the NO-ARGUMENT
            // form is modeled — an argument collides with the pointer's own base
            // register and needs the measured save/schedule (roadmap). The target is
            // the callee-pointer expression (case 3 in the parser peeled the outer
            // `*`), restricted to the measured shapes that load cleanly into r12.
            Statement::Expression(Expression::CallThrough { target, arguments }) => {
                if !arguments.is_empty() {
                    return Err(Diagnostic::error(
                        "arguments to a bare indirect call are not supported yet (roadmap)",
                    ));
                }
                if !matches!(
                    target.as_ref(),
                    Expression::Dereference { .. } | Expression::Member { .. }
                ) {
                    return Err(Diagnostic::error(
                        "this bare indirect-call target is not supported yet (roadmap)",
                    ));
                }
                self.evaluate(target, Type::UnsignedInt, 12)?;
                self.emit_indirect_branch_and_link(12);
                Ok(())
            }
            // A bare CONSTANT expression statement is a no-op — mwcc emits
            // nothing (strikers alloc's FORCE_DONT_INLINE: 176 `(void*)0;`).
            Statement::Expression(Expression::IntegerLiteral(_)) => Ok(()),
            Statement::Expression(Expression::Cast { operand, .. })
                if matches!(**operand, Expression::IntegerLiteral(_)) =>
            {
                Ok(())
            }
            Statement::Expression(expression) => {
                if self.try_emit_conditional_call_statement(expression)? {
                    Ok(())
                } else {
                    Err(Diagnostic::error(format!(
                        "only a call may be a bare statement (roadmap): {expression:?}"
                    )))
                }
            }
            Statement::Assign { name, value } if self.frame_slots.contains_key(name) => {
                if self.try_emit_frame_aggregate_virtual_assignment(name, value)? {
                    Ok(())
                } else {
                    self.emit_store(&Expression::Variable(name.clone()), value)
                }
            }
            // Reassignment is handled by value tracking; reaching here means it was
            // mixed with stores/calls, which that path defers.
            Statement::Assign { .. } => Err(Diagnostic::error(
                "local reassignment mixed with stores/calls is not supported yet (roadmap)",
            )),
            // The binary-search dispatch codegen is the next piece; switches parse
            // but defer for now (never miscompile).
            Statement::Switch { .. } => Err(Diagnostic::error(
                "switch dispatch codegen is not implemented yet (roadmap)",
            )),
            // A general if-statement (non-trailing, non-leaf, or with an else) needs
            // forward branches and basic-block scheduling — deferred for now.
            Statement::If { .. } => Err(Diagnostic::error(format!(
                "general if-statement codegen is not implemented yet (roadmap; function '{}')",
                self.output.name
            ))),
            // An early `return` inside the body needs early-return codegen (blr for
            // a leaf, a forward branch to the shared epilogue otherwise) — the
            // parser now models it, but the codegen is the next piece.
            Statement::Return(_) => Err(Diagnostic::error(
                "early-return codegen is not implemented yet (roadmap)",
            )),
            // Focused loop owners lower measured topologies; every other loop
            // still defers rather than falling through to straight-line codegen.
            Statement::Loop { .. } => {
                if self.try_emit_global_struct_member_search_loop(statement)? {
                    Ok(())
                } else {
                    Err(Diagnostic::error(
                        "loop codegen is not implemented yet (roadmap)",
                    ))
                }
            }
        }
    }

    /// evaluate() with the live-local homes visible as locations (a
    /// reassignment reads its own or a sibling's home).
    pub(crate) fn evaluate_with_live_locals(
        &mut self,
        value: &Expression,
        destination: u8,
        homes: &[(String, u8)],
    ) -> Compilation<()> {
        for (name, register) in homes {
            self.locations
                .entry(name.clone())
                .or_insert(crate::generator::Location {
                    class: crate::generator::ValueClass::General,
                    register: *register,
                    signed: true,
                    width: 32,
                    pointee: None,
                    stride: None,
                });
        }
        self.evaluate(value, Type::Int, destination)
    }

    /// Evaluate the function result. A conditional in this tail position can use a
    /// conditional return when one of its values already sits in the result register.
    pub(crate) fn evaluate_tail(
        &mut self,
        expression: &Expression,
        value_type: Type,
        result: u8,
    ) -> Compilation<()> {
        // `bool` shares one-byte storage with `unsigned char` in the compact
        // type IR, but a relational/equality/logical-not expression already
        // materializes the canonical word value 0 or 1. MWCC returns that word
        // directly; applying the unsigned-byte return truncation adds a
        // redundant `clrlwi` and can force the comparison into r0 first.
        let canonical_boolean = matches!(
            expression,
            Expression::Binary {
                operator: BinaryOperator::Less
                    | BinaryOperator::Greater
                    | BinaryOperator::LessEqual
                    | BinaryOperator::GreaterEqual
                    | BinaryOperator::Equal
                    | BinaryOperator::NotEqual,
                ..
            } | Expression::Unary {
                operator: UnaryOperator::LogicalNot,
                ..
            }
        );
        if self.return_source_fundamental
            == Some(mwcc_syntax_trees::SourceFundamentalType::Boolean)
            && canonical_boolean
        {
            return self.evaluate(expression, Type::Int, result);
        }
        // A call whose source return type is exactly the caller's return type
        // has already satisfied that type's value representation. In
        // particular, both `bool -> bool` and `unsigned char -> unsigned char`
        // forward r3 unchanged; `unsigned char -> bool` is deliberately not
        // covered because it requires boolean normalization.
        if let Expression::Call { name, .. } = expression {
            if is_narrow_int(value_type)
                && self.return_source_fundamental.is_some()
                && self.return_source_fundamental
                    == self.call_return_fundamentals.get(name).copied()
                && self.call_return_types.get(name) == Some(&value_type)
            {
                return self.evaluate(expression, Type::Int, result);
            }
        }
        if self.behavior.narrow_computed_return_style == NarrowComputedReturnStyle::FullWidthResult
            && is_narrow_int(value_type)
        {
            if let Expression::Cast {
                target_type,
                operand,
            } = expression
            {
                if target_type.width() == value_type.width() && self.leaf_info(operand).is_err() {
                    let saved = self.narrow_truncation_context;
                    self.narrow_truncation_context = true;
                    let evaluated = self.evaluate(operand, Type::Int, result);
                    self.narrow_truncation_context = saved;
                    return evaluated;
                }
            }
            if matches!(
                expression,
                Expression::Binary { .. } | Expression::Unary { .. }
            ) {
                let saved = self.narrow_truncation_context;
                self.narrow_truncation_context = true;
                let evaluated = self.evaluate(expression, Type::Int, result);
                self.narrow_truncation_context = saved;
                return evaluated;
            }
        }
        match expression {
            Expression::Conditional {
                condition,
                when_true,
                when_false,
                origin,
            } => match value_type {
                Type::Float | Type::Double => {
                    self.emit_float_conditional(condition, when_true, when_false, result, true)
                }
                _ => {
                    // ATTEMPT the select; a false-arm outside its vocabulary
                    // (a table load) uses mwcc's early-return BRANCH — the
                    // ternary is the guard form `if (cond) return T; return F`
                    // (measured on the ctype tolower shape).
                    let instructions_before = self.output.instructions.len();
                    let relocations_before = self.output.relocations.len();
                    let virtuals_before = self.next_virtual;
                    let bump_before = self.output.anonymous_label_bump;
                    let labels_before = self.labels.checkpoint();
                    match self
                        .emit_conditional(condition, when_true, when_false, result, true, *origin)
                    {
                        Ok(()) => Ok(()),
                        Err(error) => {
                            self.output.instructions.truncate(instructions_before);
                            self.output.relocations.truncate(relocations_before);
                            self.next_virtual = virtuals_before;
                            self.output.anonymous_label_bump = bump_before;
                            self.labels.rollback(labels_before);
                            // Emit the branch form DIRECTLY (a nested-ternary
                            // fall-through would recurse through the same
                            // fallback forever — defer that).
                            let Some(constant) = constant_value(when_true) else {
                                return Err(error);
                            };
                            if matches!(when_false.as_ref(), Expression::Conditional { .. }) {
                                return Err(error);
                            }
                            let (options, condition_bit) = self.emit_condition_test(condition)?;
                            let branch_index = self.output.instructions.len();
                            self.output
                                .instructions
                                .push(Instruction::BranchConditionalForward {
                                    options,
                                    condition_bit,
                                    target: 0,
                                });
                            self.load_integer_constant(result, constant);
                            self.output
                                .instructions
                                .push(Instruction::BranchToLinkRegister);
                            let next = self.output.instructions.len();
                            if let Instruction::BranchConditionalForward { target, .. } =
                                &mut self.output.instructions[branch_index]
                            {
                                *target = next;
                            }
                            self.evaluate_tail(when_false, value_type, result)
                        }
                    }
                }
            },
            Expression::Binary {
                operator: operator @ (BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr),
                left,
                right,
            } => self.emit_short_circuit(*operator, left, right, result),
            // Negated short-circuit policy is versioned: mainline applies De
            // Morgan, while build 163 materializes and inverts the written
            // logical value. Nested logical operands still defer.
            Expression::Unary {
                operator: UnaryOperator::LogicalNot,
                operand,
            } if matches!(
                operand.as_ref(),
                Expression::Binary {
                    operator: BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr,
                    ..
                }
            ) =>
            {
                let Expression::Binary {
                    operator: inner,
                    left,
                    right,
                } = operand.as_ref()
                else {
                    unreachable!()
                };
                self.emit_negated_short_circuit(*inner, left, right, result)
            }
            // A narrow return type truncates the returned value. A `(type)` cast
            // expression already yields the narrow type, so it falls through to the
            // normal path; everything else is coerced here.
            other if is_narrow_int(value_type) && !matches!(other, Expression::Cast { .. }) => {
                self.evaluate_narrow_return(other, value_type, result)
            }
            other => self.evaluate(other, value_type, result),
        }
    }

    pub(crate) fn evaluate_single_local(
        &mut self,
        local: &LocalDeclaration,
        return_expression: &Expression,
        return_type: Type,
        result: u8,
    ) -> Compilation<()> {
        let class = class_of(local.declared_type)?;
        // The single-local straight-line path needs the local's initializer; an
        // uninitialized local (its value comes from an assignment) is value-tracked.
        let initializer = local.initializer.as_ref().ok_or_else(|| {
            Diagnostic::error("an uninitialized single local is not supported here (roadmap)")
        })?;

        // `return x;` — the local is the result, so compute its initializer
        // straight into the result register.
        if matches!(return_expression, Expression::Variable(name) if *name == local.name) {
            // A signed narrow local (char/short) returned at a wider type must be
            // sign-extended — `char c = *s; return c;` is `lbz; extsb` like the direct
            // `return *s`. Evaluating the initializer at the local's own narrow type drops
            // that widening, and whether the value is already extended depends on the
            // initializer (a global narrow load appends extsb/lha; a char* deref's `lbz`
            // and a parameter leave the raw byte). Defer the not-already-extended cases
            // rather than return a zero-extended byte where a sign-extended char is meant.
            if self.signed_of(local.declared_type)
                && local.declared_type.width() < return_type.width()
                && local.declared_type.width() < 32
            {
                let initializer_extends = match initializer {
                    // A global signed-narrow load appends the extension (lbz+extsb / lha).
                    Expression::Variable(name) => self.globals.contains_key(name.as_str()),
                    // `lha` sign-extends a halfword; `lbz` does not extend a byte.
                    Expression::Dereference { .. }
                    | Expression::Index { .. }
                    | Expression::Member { .. } => local.declared_type.width() >= 16,
                    _ => false,
                };
                if !initializer_extends {
                    return Err(Diagnostic::error("a signed narrow local returned at a wider type needs a widening coercion (roadmap)"));
                }
            }
            // A NARROWING leaf initializer — `char c = a;` for a wider `a` — truncates to the
            // narrow type. Inlining it into the return drops that truncation (and the char
            // return's sign-extension): mwcc emits `extsb r3,r3` for `char f(int a){ char c =
            // a; return c; }`, ours returned the raw int. Defer the narrowing.
            if local.declared_type.width() < 32 {
                if let Ok((_, init_width, _)) = self.leaf_info(initializer) {
                    if init_width as u32 > local.declared_type.width() as u32 {
                        return Err(Diagnostic::error("a narrowing narrow local (char/short from a wider value) returned is not supported yet (roadmap)"));
                    }
                }
            }
            return self.evaluate(initializer, local.declared_type, result);
        }

        // An additively-defined local used as an operand of an addition
        // (`int t = a + b; return t + c;`) is one mwcc keeps in a register and
        // mutates in place (`add r3,r3,r4; add r3,r3,r5`); our leaf-in-scratch
        // lowering would instead reassociate it like a direct sum. Defer that exact
        // shape (a `+`-init local feeding a `+`); the allocator will later make it
        // byte-exact. Other shapes (`*` init, or a `*`/`-` use) already match.
        fn feeds_an_addition(name: &str, expression: &Expression) -> bool {
            let is_local = |operand: &Expression| matches!(operand, Expression::Variable(variable) if variable == name);
            match expression {
                Expression::CompoundLiteral { .. } => false,
                Expression::CallThrough { target, arguments } => {
                    feeds_an_addition(name, target)
                        || arguments
                            .iter()
                            .any(|argument| feeds_an_addition(name, argument))
                }
                Expression::VirtualCall {
                    object, arguments, ..
                } => {
                    feeds_an_addition(name, object)
                        || arguments
                            .iter()
                            .any(|argument| feeds_an_addition(name, argument))
                }
                Expression::AggregateLiteral(_) => false,
                Expression::PostStep { target, .. } => feeds_an_addition(name, target),
                Expression::Binary {
                    operator,
                    left,
                    right,
                } => {
                    (*operator == BinaryOperator::Add && (is_local(left) || is_local(right)))
                        || feeds_an_addition(name, left)
                        || feeds_an_addition(name, right)
                }
                Expression::Unary { operand, .. }
                | Expression::Cast { operand, .. }
                | Expression::BitFieldRead {
                    extracted: operand, ..
                }
                | Expression::IndexedUpdateValue { value: operand }
                | Expression::AddressOf { operand } => feeds_an_addition(name, operand),
                Expression::Conditional {
                    condition,
                    when_true,
                    when_false,
                    ..
                } => {
                    feeds_an_addition(name, condition)
                        || feeds_an_addition(name, when_true)
                        || feeds_an_addition(name, when_false)
                }
                Expression::Dereference { pointer } => feeds_an_addition(name, pointer),
                Expression::Index { base, index } => {
                    feeds_an_addition(name, base) || feeds_an_addition(name, index)
                }
                Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => {
                    feeds_an_addition(name, base)
                }
                Expression::Assign { target, value } => {
                    feeds_an_addition(name, target) || feeds_an_addition(name, value)
                }
                Expression::Comma { left, right } => {
                    feeds_an_addition(name, left) || feeds_an_addition(name, right)
                }
                Expression::Call { arguments, .. } => arguments
                    .iter()
                    .any(|argument| feeds_an_addition(name, argument)),
                Expression::ConstructedNew {
                    allocation,
                    arguments,
                    ..
                } => {
                    feeds_an_addition(name, allocation)
                        || arguments
                            .iter()
                            .any(|argument| feeds_an_addition(name, argument))
                }
                Expression::Variable(_)
                | Expression::IntegerLiteral(_)
                | Expression::FloatLiteral(_)
                | Expression::StringLiteral(_) => false,
            }
        }
        if matches!(
            initializer,
            Expression::Binary {
                operator: BinaryOperator::Add,
                ..
            }
        ) && feeds_an_addition(&local.name, return_expression)
        {
            return Err(Diagnostic::error("an additively-defined local used in a sum needs the register allocator to match mwcc's in-place mutation (roadmap)"));
        }
        // An ARITHMETICALLY-COMPUTED plain-`int` local used as the LEFT operand of a
        // commutative add/bitwise op whose RIGHT operand is a constant, or a variable that
        // appears in the local's initializer: mwcc anchors by liveness/register-reuse —
        // `int b=a*a; return b+a` -> `add r3,r3,r0` anchoring the still-live `a`; `return b+3`
        // -> `mullw r3,r3,r3; addi r3,r3,3` keeping b in r3 — which the source-order scratch-leaf
        // placement below does not reproduce (and for a constant right operand it even drops the
        // local, a miscompile). Defer; the register allocator (#20) makes it exact. Restricted to
        // a plain-int local with a computed (Binary) initializer: a FLOAT local (`y=a*b; return
        // y+a`, `fadds` order matches), a NARROW/LOAD-init local (`char c=*s; return c+1`, the
        // extended value is register-resident), an INDEPENDENT-register right operand (`b+c`), a
        // `*`/`-` operator, and `b+b` all place operands as mwcc does and already match.
        if matches!(local.declared_type, Type::Int | Type::UnsignedInt)
            && matches!(
                initializer,
                Expression::Binary { .. } | Expression::Unary { .. }
            )
        {
            if let Expression::Binary {
                operator,
                left,
                right,
            } = return_expression
            {
                let commutative_anchor_op = matches!(
                    operator,
                    BinaryOperator::Add
                        | BinaryOperator::BitOr
                        | BinaryOperator::BitAnd
                        | BinaryOperator::BitXor
                );
                let is_local = |operand: &Expression| matches!(operand, Expression::Variable(name) if name == &local.name);
                // A CONSTANT operand on EITHER side drops the scratch local — a miscompile: `b+3`
                // and `3+b` (b=a*a) both emit `li r3,3`. An init-reused VARIABLE diverges only when
                // the local is the LEFT operand (`b+a` -> source-order `add r3,r0,r3`, but mwcc
                // anchors the live `a`: `add r3,r3,r0`); with the local on the RIGHT (`a+b`) source
                // order already anchors the leaf and matches. A `-a`-init local folded to `-a+a`
                // (mwcc `li r3,0`) is caught by the same init-variable rule.
                if commutative_anchor_op {
                    let constant_other = (is_local(left) && constant_value(right).is_some())
                        || (is_local(right) && constant_value(left).is_some());
                    let init_variable_on_right = is_local(left)
                        && matches!(right.as_ref(), Expression::Variable(name) if count_name_occurrences(initializer, name) > 0);
                    if constant_other || init_variable_on_right {
                        return Err(Diagnostic::error("a computed local anchored by liveness in a commutative op needs the register allocator (roadmap)"));
                    }
                }
            }
        }

        // Otherwise the local lives in the scratch register and is used as a leaf.
        // That only works if the result expression does not itself need the scratch.
        if needs_scratch(return_expression) {
            return Err(Diagnostic::error(
                "local reused inside a scratch-needing expression (roadmap M1)",
            ));
        }
        let scratch = match class {
            ValueClass::General => GENERAL_SCRATCH,
            ValueClass::Float => FLOAT_SCRATCH,
        };
        self.evaluate(initializer, local.declared_type, scratch)?;
        let signed = self.signed_of(local.declared_type);
        let pointee = match local.declared_type {
            Type::Pointer(pointee) => Some(pointee),
            _ => None,
        };
        let stride = pointer_stride(local.declared_type);
        self.locations.insert(
            local.name.clone(),
            Location {
                class,
                register: scratch,
                signed,
                width: local.declared_type.width(),
                pointee,
                stride,
            },
        );
        self.evaluate(return_expression, return_type, result)
    }

    pub(crate) fn evaluate(
        &mut self,
        expression: &Expression,
        value_type: Type,
        destination: u8,
    ) -> Compilation<()> {
        // A C++ reference binding to an aggregate (`S& r = *p`) carries the
        // aggregate's address. The frontend represents references as
        // `StructPointer`, so evaluating the dereference as a scalar load would
        // incorrectly try to read an entire struct. Materialize the pointer
        // value instead; member pointers still perform their one required `lwz`.
        if matches!(value_type, Type::StructPointer { .. }) {
            if let Expression::Dereference { pointer } = expression {
                return self.evaluate_general(pointer, destination);
            }
        }
        // An `(int)` cast of an UNSIGNED-narrow or int-typed operand is a no-op
        // (the lbz/lhz load already zero-extends): unwrap it. A signed-narrow
        // operand keeps the cast (its widening is the extsb/extsh the inner
        // paths model).
        if let (
            Type::Int | Type::UnsignedInt,
            Expression::Cast {
                target_type: Type::Int | Type::UnsignedInt,
                operand,
            },
        ) = (value_type, expression)
        {
            let element = match operand.as_ref() {
                Expression::Index { base, .. } => match base.as_ref() {
                    Expression::Variable(name) => self.globals.get(name.as_str()).copied(),
                    _ => None,
                },
                _ => None,
            };
            match element {
                // An UNSIGNED narrow (or int) element zero-extends in its own
                // load (lbzx/lhzx): the cast is a no-op.
                Some(Type::UnsignedChar | Type::UnsignedShort | Type::Int | Type::UnsignedInt) => {
                    return self.evaluate(operand, value_type, destination);
                }
                // A SIGNED narrow element's widening (lbzx then extsb) is the
                // Index path's own job — the cast is a no-op wrapper here too.
                Some(Type::Char | Type::Short) => {
                    return self.evaluate(operand, value_type, destination);
                }
                _ => {}
            }
            if matches!(
                operand.as_ref(),
                Expression::Variable(_) | Expression::IntegerLiteral(_) | Expression::Binary { .. }
            ) {
                return self.evaluate(operand, value_type, destination);
            }
        }
        match value_type {
            // A `double` shares the FPR file with `float`; the float path picks the
            // double-precision instructions via is_double_value. An integer leaf in
            // a float context is an implicit int->float conversion (the same magic-
            // constant sequence as the explicit `(float)`/`(double)` cast).
            Type::Float | Type::Double => {
                // A bare float literal materializes at the CONTEXT precision: an 8-byte
                // pooled `lfd` for a double, the rounded 4-byte `lfs` for a float.
                // evaluate_float cannot know the context and always picked single,
                // which mis-typed every double-constant return (`return 0.0;`).
                if let Expression::FloatLiteral(value) = expression {
                    self.load_float_literal(destination, *value, value_type == Type::Double);
                    return Ok(());
                }
                if self.is_integer_leaf(expression) {
                    return self.emit_cast_to_float(
                        expression,
                        destination,
                        value_type == Type::Double,
                    );
                }
                // A call returning int — or an implicitly-declared callee (defaults to int),
                // the libm `w_*` wrappers `double acos(double x){ return __ieee754_acos(x); }`
                // — leaves its result in r3. Convert it to the CONTEXT precision (this branch
                // knows `value_type`, which evaluate_float does not) via the magic-bias
                // sequence, reusing the non-leaf call prologue's frame (no second stwu). mwcc
                // schedules the call-result conversion value-store-first: the call->xoris->stw
                // value chain is the critical path, so the independent bias load fills the slot
                // after. An intrinsic (`__fabs`) is not a real call and is left to evaluate_float.
                if let Expression::Call { name, arguments } = expression {
                    if !is_intrinsic_call(name)
                        && !matches!(
                            self.call_return_types.get(name),
                            Some(Type::Float | Type::Double)
                        )
                    {
                        let source = Eabi::general_result().number;
                        self.emit_call(name, arguments, None, false)?;
                        let bias_register = if destination != FLOAT_SCRATCH {
                            destination
                        } else {
                            Eabi::float_result().number
                        };
                        self.emit_int_to_float_body(
                            source,
                            destination,
                            value_type == Type::Double,
                            true,
                            bias_register,
                            crate::casts::IntToFloatSchedule::CallResult,
                        );
                        return Ok(());
                    }
                }
                // An integer memory load (`*p`, `a[i]`, `s.member` of integer type) in a
                // float context needs the loaded value run through the int->float conversion.
                // That path is not wired, so defer rather than hand it to evaluate_float,
                // which would mis-evaluate the integer as a float and load it into the GPR
                // whose NUMBER matches the float destination (f1 -> r1, clobbering the stack
                // pointer). Float-typed loads fall through to evaluate_float as before.
                // A deref/index of a leaf-variable base (int pointer, int global array) whose
                // loaded value is not float, or a direct integer struct member. Member-based
                // bases (`*p->fq`, `p->e[i]`) are left to evaluate_float — is_float_value
                // cannot resolve them, and those float loads are already byte-exact.
                let integer_memory_load = match expression {
                    Expression::Dereference { pointer } => {
                        matches!(pointer.as_ref(), Expression::Variable(_))
                            && !self.is_float_value(expression)
                    }
                    Expression::Index { base, .. } => {
                        matches!(base.as_ref(), Expression::Variable(_))
                            && !self.is_float_value(expression)
                    }
                    Expression::Member { member_type, .. } => {
                        !matches!(member_type, Type::Float | Type::Double)
                    }
                    // A plain file-scope global of INT (non-float) type read in a float context —
                    // `double f(){ return gi; }` — is an integer memory load too. Without this,
                    // evaluate_float treats it as a float global and loads it (`lwz`) into the GPR
                    // whose number matches the float destination: f1 -> r1, CLOBBERING the stack
                    // pointer. A local/param is not a memory load (excluded via `locations`).
                    Expression::Variable(name) => {
                        !self.locations.contains_key(name.as_str())
                            && matches!(self.globals.get(name.as_str()), Some(global_type) if !matches!(global_type, Type::Float | Type::Double))
                    }
                    _ => false,
                };
                if integer_memory_load {
                    return Err(Diagnostic::error("an integer memory load in a float context needs an int->float conversion (roadmap)"));
                }
                self.evaluate_float(expression, destination)
            }
            Type::Void => Err(Diagnostic::error("cannot evaluate a void expression")),
            // A float leaf in an integer context is an implicit float->int conversion
            // (the same `fctiwz` + frame bounce as the explicit `(int)` cast).
            _ => {
                if self.is_float_value(expression) {
                    return self.emit_cast_to_integer(value_type, expression, destination);
                }
                // A whole signed-`char` load promoted to `int` sign-extends the
                // loaded byte: `lbz d,…; extsb d,d`. (`lbz` zero-extends, so the
                // promotion needs the trailing `extsb`; the narrow-return path
                // calls `evaluate_general` directly and so keeps the bare `lbz`.)
                if matches!(value_type, Type::Int | Type::UnsignedInt)
                    && self.is_signed_byte_load(expression)?
                {
                    self.evaluate_general(expression, destination)?;
                    self.emit_widen(destination, destination, 8, true);
                    return Ok(());
                }
                self.evaluate_general(expression, destination)
            }
        }
    }

    /// Whether `expression` is a full-width integer leaf variable (an int/unsigned
    /// in a GPR, not a pointer or a narrow type) — the operand an implicit
    /// int->float conversion accepts.
    pub(crate) fn is_integer_leaf(&self, expression: &Expression) -> bool {
        matches!(expression, Expression::Variable(name)
            if self.locations.get(name.as_str())
                .is_some_and(|location| location.class == ValueClass::General && location.width == 32 && location.pointee.is_none()))
    }
}

fn is_single_short_circuit_call_if(function: &Function) -> bool {
    matches!(function.statements.as_slice(), [
        Statement::If {
            condition: Expression::Binary {
                operator: BinaryOperator::LogicalAnd,
                ..
            },
            then_body,
            else_body,
        }
    ] if else_body.is_empty() && then_body.iter().any(statement_has_call))
}
