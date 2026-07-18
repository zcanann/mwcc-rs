//! Pointer loads/stores, const-address access, pointer arithmetic and resolution.

#[allow(unused_imports)]
use super::*;

impl Generator {
    /// Place an operand and return the register holding it. A leaf stays in its
    /// own register. A sub-expression is computed into the destination when the
    /// consumer can fold it there (`addi`), otherwise into the scratch register —
    /// mwcc keeps `addi` operands in place but routes `rlwinm`/logical operands
    /// through `r0`. Returns `None` when a scratch operand does not fit.
    /// Emit `*pointer` — load the pointed-to value into `destination`, choosing
    /// the load by the pointee type (`lwz`/`lbz`/`lha`/`lhz`/`lfs`). The pointer
    /// must be a leaf variable holding the address; richer addressing is on the
    /// roadmap.
    pub(crate) fn emit_load_from_pointer(
        &mut self,
        pointer: &Expression,
        destination: u8,
    ) -> Compilation<()> {
        // A type-pun through a frame-resident address (`*(int*)&x`) is a plain
        // displacement load from r1.
        if let Some((pointee, offset)) = self.resolve_frame_pointer(pointer) {
            self.output
                .instructions
                .push(displacement_load(pointee, destination, 1, offset)?);
            return Ok(());
        }
        // A global pointer: load the pointer value into the destination (an SDA21
        // word load), then dereference it from there, as mwcc does.
        if let Expression::Variable(name) = pointer {
            if !self.locations.contains_key(name) {
                if let Some(Type::Pointer(pointee)) = self.globals.get(name).copied() {
                    // The pointer and the integer result share the destination, so a
                    // float pointee (which needs a separate general register for the
                    // address) is deferred rather than miscompiled.
                    if !matches!(pointee, Pointee::Float | Pointee::Double) {
                        self.emit_global_load(name, destination)?;
                        self.output.instructions.push(displacement_load(
                            pointee,
                            destination,
                            destination,
                            0,
                        )?);
                        return Ok(());
                    }
                }
            }
        }
        // `*(p + i)` / `*(p + 3)` is exactly `p[i]` / `p[3]` — mwcc emits the identical
        // `slwi; lwzx` (variable index) or displacement `lwz` (constant). Route a
        // pointer-plus-index dereference to the subscript path. The pointer operand is the
        // base (the dereferenced_width-resolvable side), the integer the index; `+` commutes.
        // Narrow char/short pointees are now handled too: dereferenced_width / pointee_of see
        // through the `p + i` pointer, so a narrow `*(p+i)` either extends correctly (a return
        // adds the extsb via is_signed_byte_load) or defers in arithmetic — like `p[i]`.
        if let Expression::Binary {
            operator: BinaryOperator::Add,
            left,
            right,
        } = pointer
        {
            if self.dereferenced_width(left).is_some() {
                return self.emit_subscript(left, right, destination);
            }
            if self.dereferenced_width(right).is_some() {
                return self.emit_subscript(right, left, destination);
            }
        }
        // `*(p - C)` is `p[-C]` — a displacement load at the negative offset. Subtract does NOT
        // commute (the pointer is always the left operand), and only a CONSTANT offset to a
        // NON-narrow pointee is routed: a variable `*(p - i)` needs a negated, scaled index
        // (`neg; slwi; lwzx`), and a char/short pointee needs the narrow machinery to see
        // through the `p - C` pointer (as it does for `p + C`) — both keep deferring.
        if let Expression::Binary {
            operator: BinaryOperator::Subtract,
            left,
            right,
        } = pointer
        {
            if let Some(constant) = constant_value(right) {
                if self.dereferenced_width(left) >= Some(32) {
                    return self.emit_subscript(
                        left,
                        &Expression::IntegerLiteral(-constant),
                        destination,
                    );
                }
            }
        }
        // `*(T *)0xADDR` — a constant-address load. When the address fits the signed 16-bit
        // displacement (high half zero) mwcc loads straight off the r0=0 base (`ld dest,
        // lo(0)`); otherwise it materializes the sign-adjusted high half with `lis dest, hi`
        // and folds the low half into the displacement (`ld dest, lo(dest)`), reusing the
        // destination as the address register. When the value itself is staged through r0,
        // materialize the address in the lowest free GPR instead: r0 in a base field means
        // literal zero on PowerPC, never the contents written by `lis`.
        if let Some((pointee, address)) = const_address_pointer(pointer) {
            if self.emit_const_address_load(pointee, address, 0, destination)? {
                return Ok(());
            }
            return Err(Diagnostic::error(
                "a constant-address load needing base reuse is not supported yet (roadmap)",
            ));
        }
        let (pointee, address) = self.resolve_pointer(pointer)?;
        self.output
            .instructions
            .push(displacement_load(pointee, destination, address, 0)?);
        Ok(())
    }

    /// Emit `base->field` — a displacement load from the struct pointer's register
    /// at the member's offset, choosing the load by the member type. The base must
    /// be a struct-pointer leaf variable (chained/complex bases are roadmap).
    /// Load from constant `address + offset` (a `*(T *)C` deref or a `(*(struct S *)C).field`
    /// member). Materializes the address with the `lis hi` / displacement-`lo` split, folding
    /// the member offset into the displacement; a zero high half loads off the r0=0 base. Returns
    /// `false` (caller defers) when the displacement overflows i16 or a second access needs
    /// mwcc's look-ahead base scheduling.
    pub(crate) fn emit_const_address_load(
        &mut self,
        pointee: Pointee,
        address: u32,
        offset: u16,
        destination: u8,
    ) -> Compilation<bool> {
        let (high, low) = split_address(address);
        let Some(displacement) = (low as i32)
            .checked_add(offset as i32)
            .and_then(|d| i16::try_from(d).ok())
        else {
            return Ok(false);
        };
        // Only the FIRST constant-address access in a function is byte-exact. mwcc handles a run
        // of them by allocating all the bases up front (chosen by look-ahead over every value)
        // and scheduling them together — keystone-level register allocation. So a second access
        // of any kind defers rather than emit a fresh, mis-scheduled sequence.
        if !self.const_address_bases.is_empty() {
            return Ok(false);
        }
        self.const_address_bases.insert(high);
        if high == 0 {
            self.output.instructions.push(displacement_load(
                pointee,
                destination,
                0,
                displacement,
            )?);
        } else {
            let base = self.address_base_for_load_destination(destination)?;
            self.output
                .instructions
                .push(Instruction::load_immediate_shifted(base, high));
            self.output.instructions.push(displacement_load(
                pointee,
                destination,
                base,
                displacement,
            )?);
        }
        Ok(true)
    }

    /// Select the address register for a load which normally coalesces its base
    /// with its destination. r0 is the expression scratch, but in a D-form load's
    /// base field it denotes constant zero. A scratch-destination load therefore
    /// needs the lowest free GPR; callers expose live siblings through `reserved`,
    /// so this yields r3 normally, r4 while r3 is live, and so on.
    pub(crate) fn address_base_for_load_destination(&self, destination: u8) -> Compilation<u8> {
        if destination == GENERAL_SCRATCH {
            self.lowest_free_general()
        } else {
            Ok(destination)
        }
    }

    /// Store `value` to constant `address + offset` (a `*(T *)C = v` or `(*(struct S *)C).f = v`).
    /// The address base is materialized before the value and kept clear of the value's input
    /// registers, mirroring the absolute global store. Returns `false` (caller defers) when the
    /// displacement overflows i16.
    pub(crate) fn emit_const_address_store(
        &mut self,
        pointee: Pointee,
        address: u32,
        offset: u16,
        value: &Expression,
    ) -> Compilation<bool> {
        let (high, low) = split_address(address);
        let Some(displacement) = (low as i32)
            .checked_add(offset as i32)
            .and_then(|d| i16::try_from(d).ok())
        else {
            return Ok(false);
        };
        // This path lays the base `lis` down BEFORE the value (below), which matches mwcc
        // ONLY when the value is a register-resident LEAF that needs no pre-computation —
        // then mwcc also emits just `lis base; store`. For ANY value that first emits
        // instructions (a constant `li`, a global load, a computed expression, a call, or
        // an int<->float conversion), mwcc emits the VALUE first and materializes the base
        // AFTER it, REUSING a GPR the value freed (`add r0,r3,r4; lis r3; stw r0,d(r3)`) —
        // a look-ahead base allocation this base-first path does not model (keystone-level).
        // Defer those rather than emit the wrong base register/order (measured DIFFs across
        // `= a+b`, `= gi`, `= 5`, `= (float)int_x` on the GX write-gather-pipe and plain
        // const-address stores). A same-class width cast of a register leaf (`(u8)x` ->
        // `stb`) still stores from that register, so it stays a leaf.
        let is_register_leaf = match value {
            Expression::Variable(name) => self.locations.contains_key(name.as_str()),
            Expression::Cast {
                target_type,
                operand,
            } => {
                matches!(operand.as_ref(), Expression::Variable(name) if self.locations.contains_key(name.as_str()))
                    && (matches!(target_type, Type::Float | Type::Double)
                        == self.is_float_value(operand))
            }
            _ => false,
        };
        if !is_register_leaf {
            return Ok(false);
        }
        // Only the FIRST constant-address access in a function is byte-exact; a second of any
        // kind needs mwcc's look-ahead base allocation and scheduling (keystone-level). Defer.
        if !self.const_address_bases.is_empty() {
            return Ok(false);
        }
        self.const_address_bases.insert(high);
        if high == 0 {
            let source = self.place_store_value(value, pointee)?;
            self.output
                .instructions
                .push(displacement_store(pointee, source, 0, displacement)?);
            return Ok(true);
        }
        // Phase D: the const-address base is a virtual. place_store_value between its
        // definition and use picks PHYSICAL registers; the reserve marker keeps the
        // legacy chooser away from the virtual's field value, and the allocator sees
        // any physical it picks as pinned inside the base's range.
        let base = self.fresh_virtual_general();
        let restore = self.reserved.insert(base);
        self.output
            .instructions
            .push(Instruction::load_immediate_shifted(base, high));
        let source = self.place_store_value(value, pointee)?;
        if restore {
            self.reserved.remove(&base);
        }
        self.output
            .instructions
            .push(displacement_store(pointee, source, base, displacement)?);
        Ok(true)
    }

    /// The pointee size of a leaf pointer variable, when greater than one byte
    /// (so its arithmetic needs scaling). A byte pointer returns `None` — its
    /// arithmetic is a plain add.
    pub(crate) fn scaled_pointer(&self, operand: &Expression) -> Option<u16> {
        if let Expression::Variable(name) = operand {
            if let Some(location) = self.locations.get(name) {
                // A struct pointer scales by the struct's byte size; a scalar pointer
                // by its pointee size (a byte element needs no scaling, so > 1).
                if let Some(stride) = location.stride {
                    return Some(stride);
                }
                let size = location.pointee?.size();
                if size > 1 {
                    return Some(size as u16);
                }
            }
        }
        None
    }

    /// The (register, element size) of a pointer operand for arithmetic: a leaf
    /// pointer wider than a byte, or an array member at offset 0 (which decays to a
    /// pointer in its base register). A byte leaf pointer returns `None` (its
    /// arithmetic is a plain add handled elsewhere); a byte *array* member is
    /// handled here, since it is not a plain leaf.
    pub(crate) fn pointer_arithmetic_base(
        &mut self,
        operand: &Expression,
    ) -> Compilation<Option<(u8, u16)>> {
        if let Expression::MemberAddress {
            base,
            offset: 0,
            element,
        } = operand
        {
            let register = self.member_base_register(base)?;
            return Ok(Some((register, u16::from(element.size()))));
        }
        if let Some(size) = self.scaled_pointer(operand) {
            return Ok(Some((self.general_register_of_leaf(operand)?, size)));
        }
        Ok(None)
    }

    /// The register and pointee size of a leaf pointer variable, with no side
    /// effects (just the home register). Used to recognize `ptr - ptr`.
    pub(crate) fn pointer_leaf_register_size(&self, operand: &Expression) -> Option<(u8, u16)> {
        if let Expression::Variable(name) = operand {
            let location = self.locations.get(name)?;
            if let Some(stride) = location.stride {
                return Some((location.register, stride));
            }
            return Some((location.register, location.pointee?.size() as u16));
        }
        None
    }

    /// Try to emit `pointer ± integer` with the integer scaled by the pointee
    /// size. Returns `false` for non-pointer (or byte leaf-pointer) operands.
    pub(crate) fn try_emit_pointer_arithmetic(
        &mut self,
        operator: BinaryOperator,
        left: &Expression,
        right: &Expression,
        destination: u8,
    ) -> Compilation<bool> {
        // `ptr - ptr` (same pointee) is the element-count difference: the byte
        // difference (`subf`) divided by the element size — a signed power-of-two
        // divide (`srawi; addze`) for sizes above one byte, just the difference for
        // a byte element.
        if operator == BinaryOperator::Subtract {
            if let (Some((left_register, size)), Some((right_register, right_size))) = (
                self.pointer_leaf_register_size(left),
                self.pointer_leaf_register_size(right),
            ) {
                if size == right_size {
                    if !size.is_power_of_two() {
                        // A difference by a non-power-of-two struct stride needs the
                        // magic-number divide mwcc emits; defer rather than mis-scale.
                        return Ok(false);
                    }
                    match size.trailing_zeros() {
                        // byte element: the difference is the element count.
                        0 => self.output.instructions.push(Instruction::SubtractFrom {
                            d: destination,
                            a: right_register,
                            b: left_register,
                        }),
                        // 2-byte element: signed divide by 2 (`srwi; add; srawi 1`).
                        1 => {
                            self.output.instructions.push(Instruction::SubtractFrom {
                                d: destination,
                                a: right_register,
                                b: left_register,
                            });
                            self.output.instructions.push(
                                Instruction::ShiftRightLogicalImmediate {
                                    a: GENERAL_SCRATCH,
                                    s: destination,
                                    shift: 31,
                                },
                            );
                            self.output.instructions.push(Instruction::Add {
                                d: GENERAL_SCRATCH,
                                a: GENERAL_SCRATCH,
                                b: destination,
                            });
                            self.output.instructions.push(
                                Instruction::ShiftRightAlgebraicImmediate {
                                    a: destination,
                                    s: GENERAL_SCRATCH,
                                    shift: 1,
                                },
                            );
                        }
                        // larger power-of-two element: signed divide via `srawi; addze`.
                        k => {
                            self.output.instructions.push(Instruction::SubtractFrom {
                                d: GENERAL_SCRATCH,
                                a: right_register,
                                b: left_register,
                            });
                            self.output.instructions.push(
                                Instruction::ShiftRightAlgebraicImmediate {
                                    a: GENERAL_SCRATCH,
                                    s: GENERAL_SCRATCH,
                                    shift: k as u8,
                                },
                            );
                            self.output
                                .instructions
                                .push(Instruction::AddToZeroExtended {
                                    d: destination,
                                    a: GENERAL_SCRATCH,
                                });
                        }
                    }
                    return Ok(true);
                }
            }
        }
        // Identify the pointer and integer operands (`i + p` is commutative).
        let (pointer_register, size, integer) =
            if let Some((register, size)) = self.pointer_arithmetic_base(left)? {
                (register, size, right)
            } else if operator == BinaryOperator::Add {
                match self.pointer_arithmetic_base(right)? {
                    Some((register, size)) => (register, size, left),
                    None => return Ok(false),
                }
            } else {
                return Ok(false);
            };
        // A constant index folds its scaled value into an `addi`.
        if let Some(constant) = constant_value(integer) {
            let scaled = constant * size as i64;
            let immediate = i16::try_from(if operator == BinaryOperator::Subtract {
                -scaled
            } else {
                scaled
            })
            .map_err(|_| Diagnostic::error("pointer offset out of range (roadmap)"))?;
            self.output.instructions.push(Instruction::AddImmediate {
                d: destination,
                a: pointer_register,
                immediate,
            });
            return Ok(true);
        }
        let integer_register = self.general_register_of_leaf(integer)?;
        // Scale the index by the element size: a power-of-two element shifts (`slwi`),
        // any other size (a struct stride like 12) multiplies (`mulli`); a byte element
        // needs neither.
        let scaled_register = if size > 1 {
            if size.is_power_of_two() {
                self.output
                    .instructions
                    .push(Instruction::ShiftLeftImmediate {
                        a: GENERAL_SCRATCH,
                        s: integer_register,
                        shift: size.trailing_zeros() as u8,
                    });
            } else {
                let immediate = i16::try_from(size)
                    .map_err(|_| Diagnostic::error("pointer stride out of range (roadmap)"))?;
                self.output
                    .instructions
                    .push(Instruction::MultiplyImmediate {
                        d: GENERAL_SCRATCH,
                        a: integer_register,
                        immediate,
                    });
            }
            GENERAL_SCRATCH
        } else {
            integer_register
        };
        match operator {
            BinaryOperator::Add => self.output.instructions.push(Instruction::Add {
                d: destination,
                a: pointer_register,
                b: scaled_register,
            }),
            // `p - i`: `subf d, scaled, p` computes `p - scaled`.
            BinaryOperator::Subtract => self.output.instructions.push(Instruction::SubtractFrom {
                d: destination,
                a: scaled_register,
                b: pointer_register,
            }),
            _ => unreachable!("caller restricts to add/subtract"),
        }
        Ok(true)
    }

    /// `(pointee, address register)` for a pointer leaf variable.
    pub(crate) fn pointer_leaf(&self, base: &Expression) -> Compilation<(Pointee, u8)> {
        let name = leaf_name(base).ok_or_else(|| {
            Diagnostic::error("pointer access needs a pointer variable (roadmap)")
        })?;
        let location = self
            .locations
            .get(name)
            .ok_or_else(|| Diagnostic::error(format!("unknown variable '{name}'")))?;
        let pointee = location
            .pointee
            .ok_or_else(|| Diagnostic::error(format!("'{name}' is not a pointer")))?;
        Ok((pointee, location.register))
    }

    /// Resolve a pointer expression to its (pointee, address register), emitting
    /// any load needed to materialize the address. A leaf pointer variable needs
    /// nothing; a pointer-typed struct member (`*p->q`) loads the pointer value
    /// into the base's register first, reusing it as mwcc does.
    pub(crate) fn resolve_pointer(&mut self, base: &Expression) -> Compilation<(Pointee, u8)> {
        // `**pp` — a double dereference through a word-pointer-to-pointer (`int **`,
        // `unsigned **`). The inner `*pp` loads the inner pointer VALUE (a word) into
        // the leaf's OWN register, as mwcc does (`lwz rN,0(rN)`); the returned address
        // register is that same register, so the outer load lands on the second
        // `lwz rN,0(rN)`. Only `WordPointer` reaches here — a narrow (`char **`) or
        // float inner keeps the opaque `Pointer` and still defers, since its `**pp`
        // would need `lbz`/`lfs` rather than a word load.
        if let Expression::Dereference { pointer: inner } = base {
            if let Some(name) = leaf_name(inner) {
                if let Some(location) = self.locations.get(name) {
                    if location.pointee == Some(Pointee::WordPointer) {
                        let register = location.register;
                        self.output.instructions.push(Instruction::LoadWord {
                            d: register,
                            a: register,
                            offset: 0,
                        });
                        // The second dereference yields the tracked 32-bit word (lwz).
                        return Ok((Pointee::UnsignedInt, register));
                    }
                }
            }
        }
        // `*(T*)p` — a pointer cast reinterprets the address; the load/store type is the cast's
        // target POINTEE (`*(int*)p` -> lwz, `*(short*)p` -> lha, `*(char*)p` -> lbz), the address a
        // leaf pointer operand (whose own pointee, e.g. `void*`, is irrelevant to the access).
        if let Expression::Cast {
            target_type: Type::Pointer(pointee),
            operand,
        } = base
        {
            if let Some(register) = leaf_name(operand).and_then(|name| self.lookup_general(name)) {
                return Ok((*pointee, register));
            }
        }
        if let Some((member_base, offset, member_type)) = as_member(base) {
            let pointee = match member_type {
                Type::Pointer(pointee) => pointee,
                _ => return Err(Diagnostic::error("dereferenced member is not a pointer")),
            };
            let register = self.member_base_register(member_base)?;
            self.output.instructions.push(Instruction::LoadWord {
                d: register,
                a: register,
                offset: offset as i16,
            });
            return Ok((pointee, register));
        }
        self.pointer_leaf(base)
    }
}
